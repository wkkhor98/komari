use std::fmt::{self, Display};

use log::{debug, info, warn};
use opencv::core::Rect;

use crate::{
    bridge::KeyKind,
    ecs::Resources,
    player::{
        Player, PlayerAction, PlayerEntity, next_action,
        timeout::{Lifecycle, Timeout, next_timeout_lifecycle},
    },
    solvers::parse_captcha_chars,
};

const CHECK_INTERVAL: u64 = 30;
const TYPING_INTERVAL: u32 = 8;
/// ~5 seconds at 30 FPS to wait for success/failure confirmation.
const VERIFY_TIMEOUT: u32 = 150;

/// Representing the current state of text captcha solving.
#[derive(Debug, Clone, Copy, Default)]
pub enum State {
    #[default]
    Waiting,
    Reading,
    Typing(Timeout, usize),
    Verifying(Timeout),
    Completed,
}

#[derive(Clone, Debug, Default)]
pub struct SolvingCaptcha {
    state: State,
    dialog_rect: Rect,
    chars: Vec<(bool, KeyKind)>,
}

impl Display for SolvingCaptcha {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self.state {
            State::Waiting => write!(f, "Waiting"),
            State::Reading => write!(f, "Reading"),
            State::Typing(_, index) => write!(f, "Typing({index})"),
            State::Verifying(_) => write!(f, "Verifying"),
            State::Completed => write!(f, "Completed"),
        }
    }
}

/// Updates the [`Player::SolvingCaptcha`] contextual state.
///
/// Note: This state does not use any [`Task`], so all detections are blocking.
pub fn update_solving_captcha_state(resources: &mut Resources, player: &mut PlayerEntity) {
    let Player::SolvingCaptcha(mut solving_captcha) = player.state.clone() else {
        panic!("state is not solving captcha");
    };

    match solving_captcha.state {
        State::Waiting => update_waiting(resources, &mut solving_captcha),
        State::Reading => update_reading(resources, &mut solving_captcha),
        State::Typing(_, _) => update_typing(resources, &mut solving_captcha),
        State::Verifying(_) => update_verifying(resources, &mut solving_captcha),
        State::Completed => unreachable!(),
    }

    let player_next_state = if matches!(solving_captcha.state, State::Completed) {
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
            solving_captcha.state = State::Reading;
        }
        Err(e) => {
            debug!(target: "backend/player", "captcha dialog not found: {e}");
            solving_captcha.state = State::Completed;
        }
    }
}

fn update_reading(resources: &mut Resources, solving_captcha: &mut SolvingCaptcha) {
    match resources
        .detector()
        .detect_lie_detector_captcha_text(solving_captcha.dialog_rect)
    {
        Ok(text) => {
            info!(target: "backend/player", "captcha OCR result: '{text}'");
            let chars = parse_captcha_chars(&text);
            if chars.is_empty() {
                warn!(target: "backend/player", "captcha text '{text}' parsed to no typeable keys, giving up");
                solving_captcha.state = State::Completed;
            } else {
                info!(target: "backend/player", "captcha typing {} chars", chars.len());
                solving_captcha.chars = chars;
                solving_captcha.state = State::Typing(Timeout::default(), 0);
            }
        }
        Err(e) => {
            warn!(target: "backend/player", "captcha OCR failed: {e}");
            solving_captcha.state = State::Completed;
        }
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
        solving_captcha.state = State::Completed;
        return;
    }

    match next_timeout_lifecycle(timeout, VERIFY_TIMEOUT) {
        Lifecycle::Started(timeout) | Lifecycle::Updated(timeout) => {
            if let Ok(dialog_rect) = resources.detector().detect_lie_detector_captcha() {
                info!(target: "backend/player", "captcha failed — new captcha at {dialog_rect:?}, retrying");
                solving_captcha.dialog_rect = dialog_rect;
                solving_captcha.state = State::Reading;
            } else {
                solving_captcha.state = State::Verifying(timeout);
            }
        }
        Lifecycle::Ended => {
            warn!(target: "backend/player", "captcha verify timed out, giving up");
            solving_captcha.state = State::Completed;
        }
    }
}
