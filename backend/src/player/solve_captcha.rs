use std::{
    cell::RefCell,
    fmt::{self, Display},
    rc::Rc,
    sync::atomic::{AtomicBool, AtomicU8, Ordering},
};

use anyhow::Result;
use log::{debug, info, warn};
use opencv::core::Rect;

#[cfg(debug_assertions)]
use crate::ecs::RecordingHandle;
use crate::{
    bridge::KeyKind,
    ecs::Resources,
    notification::NotificationKind,
    operation::OperationState,
    player::{
        Player, PlayerAction, PlayerEntity, next_action,
        timeout::{Lifecycle, Timeout, next_timeout_lifecycle},
    },
    solvers::parse_captcha_chars,
    task::{Task, Update},
};

static FAIL_COUNT: AtomicU8 = AtomicU8::new(0);
static NOTIFIED: AtomicBool = AtomicBool::new(false);

const CHECK_INTERVAL: u64 = 30;
const TYPING_INTERVAL: u32 = 8;
/// ~0.5 seconds at 30 FPS to wait after dialog detected before pressing Escape.
const INITIAL_DELAY: u32 = 15;
/// ~3 seconds at 30 FPS to wait after image appears before OCR.
const SETTLE_DELAY: u32 = 90;
/// ~5 seconds at 30 FPS to wait for success/failure confirmation.
const VERIFY_TIMEOUT: u32 = 150;

/// Representing the current state of text captcha solving.
#[derive(Clone, Copy, Debug, Default)]
pub enum State {
    #[default]
    Waiting,
    /// Waiting 3 seconds after first detection before pressing Escape.
    Delaying(Timeout),
    /// Waiting for the captcha dialog to appear after Escape was pressed.
    WaitingForImage,
    /// Waiting 0.5 seconds after image appeared before OCR.
    Settling(Timeout),
    /// Gemini OCR HTTP call is in flight; task is held in [`SolvingCaptcha::ocr_task`].
    Ocring,
    Typing(Timeout, usize),
    Verifying(Timeout),
    Completed,
}

#[derive(Clone, Debug, Default)]
pub struct SolvingCaptcha {
    state: State,
    dialog_rect: Rect,
    chars: Vec<(bool, KeyKind)>,
    ocr_task: Rc<RefCell<Option<Task<Result<(String, Vec<u8>)>>>>>,
    #[cfg(debug_assertions)]
    recording: Option<Rc<RefCell<RecordingHandle>>>,
}

impl Display for SolvingCaptcha {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self.state {
            State::Waiting => write!(f, "Waiting"),
            State::Delaying(_) => write!(f, "Delaying"),
            State::WaitingForImage => write!(f, "WaitingForImage"),
            State::Settling(_) => write!(f, "Settling"),
            State::Ocring => write!(f, "Ocring"),
            State::Typing(_, index) => write!(f, "Typing({index})"),
            State::Verifying(_) => write!(f, "Verifying"),
            State::Completed => write!(f, "Completed"),
        }
    }
}

/// Updates the [`Player::SolvingCaptcha`] contextual state.
pub fn update_solving_captcha_state(resources: &mut Resources, player: &mut PlayerEntity) {
    let Player::SolvingCaptcha(mut solving_captcha) = player.state.clone() else {
        panic!("state is not solving captcha");
    };

    #[cfg(debug_assertions)]
    if let Some(handle) = solving_captcha.recording.as_ref() {
        handle.borrow_mut().write(resources.detector());
    }

    match solving_captcha.state {
        State::Waiting => update_waiting(resources, &mut solving_captcha),
        State::Delaying(_) => update_delaying(resources, &mut solving_captcha),
        State::WaitingForImage => update_waiting_for_image(resources, &mut solving_captcha),
        State::Settling(_) => update_settling(resources, &mut solving_captcha),
        State::Ocring => update_ocring(resources, &mut solving_captcha),
        State::Typing(_, _) => update_typing(resources, &mut solving_captcha),
        State::Verifying(_) => update_verifying(resources, &mut solving_captcha),
        State::Completed => unreachable!(),
    }

    let player_next_state = if matches!(solving_captcha.state, State::Completed) {
        #[cfg(debug_assertions)]
        {
            solving_captcha.recording = None;
        }
        Player::Idle
    } else {
        Player::SolvingCaptcha(solving_captcha)
    };

    match next_action(&player.context) {
        Some(PlayerAction::SolveCaptcha) => {
            if matches!(player_next_state, Player::Idle) {
                player.context.clear_action_completed();
            }
            player.state = player_next_state;
        }
        Some(_) => unreachable!(),
        None => player.state = Player::Idle,
    }
}

fn update_waiting(resources: &mut Resources, solving_captcha: &mut SolvingCaptcha) {
    if !resources.tick.is_multiple_of(CHECK_INTERVAL) {
        return;
    }
    match resources.detector().detect_lie_detector_captcha() {
        Ok(dialog_rect) => {
            info!(target: "backend/player", "captcha dialog detected at {dialog_rect:?}");
            solving_captcha.dialog_rect = dialog_rect;
            solving_captcha.state = State::Delaying(Timeout::default());
            if !NOTIFIED.swap(true, Ordering::Relaxed) {
                resources
                    .notification
                    .schedule_notification(NotificationKind::LieDetectorCaptchaAppear);
            }
            #[cfg(debug_assertions)]
            if resources.debug.auto_record_captcha {
                use opencv::core::MatTraitConst;
                let size = resources.detector().mat().size().unwrap();
                solving_captcha.recording =
                    Some(Rc::new(RefCell::new(resources.debug.new_recording(size))));
            }
        }
        Err(e) => {
            debug!(target: "backend/player", "captcha dialog not found: {e}");
            solving_captcha.state = State::Completed;
        }
    }
}

fn update_delaying(resources: &mut Resources, solving_captcha: &mut SolvingCaptcha) {
    let State::Delaying(timeout) = solving_captcha.state else {
        panic!("solving captcha state is not delaying");
    };

    match next_timeout_lifecycle(timeout, INITIAL_DELAY) {
        Lifecycle::Started(timeout) | Lifecycle::Updated(timeout) => {
            solving_captcha.state = State::Delaying(timeout);
        }
        Lifecycle::Ended => {
            debug!(target: "backend/player", "captcha initial delay done, pressing Escape to clear input");
            resources.input.send_key(KeyKind::Esc);
            solving_captcha.state = State::WaitingForImage;
        }
    }
}

fn update_waiting_for_image(resources: &mut Resources, solving_captcha: &mut SolvingCaptcha) {
    match resources.detector().detect_lie_detector_captcha_image() {
        Ok(dialog_rect) => {
            info!(target: "backend/player", "captcha dialog fully ready at {dialog_rect:?}, settling before OCR");
            solving_captcha.dialog_rect = dialog_rect;
            solving_captcha.state = State::Settling(Timeout::default());
        }
        Err(e) => {
            debug!(target: "backend/player", "captcha dialog not yet ready: {e}");
        }
    }
}

fn update_settling(resources: &mut Resources, solving_captcha: &mut SolvingCaptcha) {
    let State::Settling(timeout) = solving_captcha.state else {
        panic!("solving captcha state is not settling");
    };

    match next_timeout_lifecycle(timeout, SETTLE_DELAY) {
        Lifecycle::Started(timeout) | Lifecycle::Updated(timeout) => {
            solving_captcha.state = State::Settling(timeout);
        }
        Lifecycle::Ended => {
            let detector = resources.detector_cloned();
            let dialog_rect = solving_captcha.dialog_rect;
            // reqwest::blocking must not run on a Tokio runtime thread, so we
            // hand it off to a dedicated std::thread and bridge the result back
            // via a oneshot channel awaited inside a Task::spawn async block.
            let task = Task::spawn(async move {
                let (tx, rx) = tokio::sync::oneshot::channel();
                std::thread::spawn(move || {
                    let _ = tx.send(detector.detect_lie_detector_captcha_text(dialog_rect));
                });
                rx.await
                    .unwrap_or_else(|_| Err(anyhow::anyhow!("OCR thread dropped sender")))
            });
            *solving_captcha.ocr_task.borrow_mut() = Some(task);
            solving_captcha.state = State::Ocring;
        }
    }
}

fn update_ocring(resources: &mut Resources, solving_captcha: &mut SolvingCaptcha) {
    let update = {
        let mut guard = solving_captcha.ocr_task.borrow_mut();
        match guard.as_mut().and_then(|t| t.poll_inner()) {
            Some(Ok(text)) => {
                *guard = None;
                Update::Ok(text)
            }
            Some(Err(e)) => {
                *guard = None;
                Update::Err(e)
            }
            None => Update::Pending,
        }
    };

    match update {
        Update::Ok((text, png_bytes)) => {
            info!(target: "backend/player", "captcha OCR result: '{text}'");
            resources.notification.schedule_notification_with_image(
                NotificationKind::LieDetectorCaptchaImage,
                png_bytes,
            );
            let chars = parse_captcha_chars(&text);
            if chars.is_empty() {
                warn!(target: "backend/player", "captcha text '{text}' parsed to no typeable keys, going back to wait for image");
                resources
                    .notification
                    .schedule_notification(NotificationKind::LieDetectorCaptchaOcrFailed);
                solving_captcha.state = State::WaitingForImage;
            } else {
                info!(target: "backend/player", "captcha typing {} chars", chars.len());
                solving_captcha.chars = chars;
                solving_captcha.state = State::Typing(Timeout::default(), 0);
            }
        }
        Update::Err(e) => {
            warn!(target: "backend/player", "captcha OCR failed: {e}, going back to wait for image");
            resources
                .notification
                .schedule_notification(NotificationKind::LieDetectorCaptchaOcrFailed);
            solving_captcha.state = State::WaitingForImage;
        }
        Update::Pending => {}
    }
}

fn update_typing(resources: &mut Resources, solving_captcha: &mut SolvingCaptcha) {
    let State::Typing(timeout, index) = solving_captcha.state else {
        panic!("solving captcha state is not typing");
    };

    match next_timeout_lifecycle(timeout, TYPING_INTERVAL) {
        Lifecycle::Started(timeout) => {
            let (needs_shift, key) = solving_captcha.chars[index];
            debug!(target: "backend/player", "captcha typing [{}/{}] {key:?} shift={needs_shift}", index + 1, solving_captcha.chars.len());
            if needs_shift {
                resources.input.send_key_down(KeyKind::Shift);
            }
            resources.input.send_key(key);
            if needs_shift {
                resources.input.send_key_up(KeyKind::Shift);
            }
            solving_captcha.state = State::Typing(timeout, index);
        }
        Lifecycle::Ended => {
            if index + 1 < solving_captcha.chars.len() {
                solving_captcha.state = State::Typing(Timeout::default(), index + 1);
            } else {
                debug!(target: "backend/player", "captcha all chars typed, pressing Enter");
                resources.input.send_key(KeyKind::Enter);
                solving_captcha.state = State::Verifying(Timeout::default());
            }
        }
        Lifecycle::Updated(timeout) => {
            solving_captcha.state = State::Typing(timeout, index);
        }
    }
}

fn update_verifying(resources: &mut Resources, solving_captcha: &mut SolvingCaptcha) {
    let State::Verifying(timeout) = solving_captcha.state else {
        panic!("solving captcha state is not verifying");
    };

    if resources.detector().detect_lie_detector_captcha_success() {
        info!(target: "backend/player", "captcha solved successfully");
        FAIL_COUNT.store(0, Ordering::Relaxed);
        NOTIFIED.store(false, Ordering::Relaxed);
        resources.input.send_key(KeyKind::Enter);
        resources
            .notification
            .schedule_notification(NotificationKind::LieDetectorCaptchaSolved);
        solving_captcha.state = State::Completed;
        return;
    }

    if resources.detector().detect_lie_detector_captcha_failure() {
        let fails = FAIL_COUNT.fetch_add(1, Ordering::Relaxed) + 1;
        resources.input.send_key(KeyKind::Enter);
        if fails >= 2 {
            warn!(target: "backend/player", "captcha failed {fails} times, stopping bot");
            FAIL_COUNT.store(0, Ordering::Relaxed);
            NOTIFIED.store(false, Ordering::Relaxed);
            resources
                .notification
                .schedule_notification(NotificationKind::LieDetectorCaptchaFailed);
            resources.operation.state = OperationState::Halting;
            solving_captcha.state = State::Completed;
        } else {
            info!(target: "backend/player", "captcha failed (attempt {fails}), waiting for new captcha image");
            solving_captcha.state = State::WaitingForImage;
        }
        return;
    }

    match next_timeout_lifecycle(timeout, VERIFY_TIMEOUT) {
        Lifecycle::Started(timeout) | Lifecycle::Updated(timeout) => {
            solving_captcha.state = State::Verifying(timeout);
        }
        Lifecycle::Ended => {
            warn!(target: "backend/player", "captcha verify timed out, giving up");
            solving_captcha.state = State::Completed;
        }
    }
}
