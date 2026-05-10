# Captcha OCR — Replace ONNX Recognizer with Gemini Flash API

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Replace the broken ONNX-based `ocr_captcha_region` with a call to Google Gemini 2.0 Flash, keeping all other OCR (player name, HP bar) and the captcha state machine completely untouched.

**Architecture:** A new `GeminiClient` struct in `backend/src/gemini.rs` encodes the cropped Mat as PNG, POSTs it to the Gemini API, and returns the cleaned alphanumeric string. `ocr_captcha_region` in `detect.rs` is the only function that changes — its signature stays identical. All other callers of `extract_texts` are unaffected.

**Tech Stack:** Rust, `reqwest` (blocking HTTP), `serde_json`, `base64` (all already in `Cargo.toml` except `blocking`/`json` reqwest features), `opencv::imgcodecs::imencode`, `anyhow`

---

## File Map

| File | Action | Responsibility |
|---|---|---|
| `backend/Cargo.toml` | Modify | Add `blocking`, `json` features to `reqwest` |
| `backend/src/gemini.rs` | Create | `GeminiClient`: API key, HTTP client, PNG→base64→POST→parse |
| `backend/src/lib.rs` | Modify | Add `mod gemini;` |
| `backend/src/detect.rs` | Modify | Replace body of `ocr_captcha_region` to use `GeminiClient` |
| `backend/examples/test_captcha_ocr.rs` | Modify | Document `GEMINI_API_KEY` requirement, skip gracefully if unset |

---

## Task 1: Add reqwest features

**Files:**
- Modify: `backend/Cargo.toml`

- [ ] **Step 1: Update reqwest dependency**

Open `backend/Cargo.toml`. Find line:
```toml
reqwest = { version = "0.12.20", features = ["multipart"] }
```
Change to:
```toml
reqwest = { version = "0.12.20", features = ["multipart", "blocking", "json"] }
```

- [ ] **Step 2: Verify it compiles**

```powershell
cargo check -p backend
```
Expected: no errors (blocking and json features are standard reqwest features).

---

## Task 2: Create GeminiClient

**Files:**
- Create: `backend/src/gemini.rs`

- [ ] **Step 1: Write the failing test first**

Create `backend/src/gemini.rs` with only the test initially:

```rust
use anyhow::{Result, bail};
use base64::{Engine as _, engine::general_purpose::STANDARD as BASE64};
use reqwest::blocking::Client;
use serde_json::json;

pub struct GeminiClient {
    client: Client,
    api_key: String,
}

impl GeminiClient {
    pub fn new() -> Result<Self> {
        todo!()
    }

    pub fn recognize(&self, _png_bytes: &[u8]) -> Result<String> {
        todo!()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn new_returns_err_when_api_key_missing() {
        // Ensure the env var is absent for this test
        std::env::remove_var("GEMINI_API_KEY");
        let result = GeminiClient::new();
        assert!(result.is_err(), "expected Err when GEMINI_API_KEY is not set");
    }
}
```

- [ ] **Step 2: Add `mod gemini;` to lib.rs**

Open `backend/src/lib.rs`. Add after the existing `mod debug;` line:
```rust
mod gemini;
```

The full module block (lines 27–51) should now include `mod gemini;` in alphabetical order:
```rust
mod array;
mod bridge;
mod buff;
mod database;
mod debug;
mod detect;
mod ecs;
mod gemini;   // <-- add this line
mod grpc;
mod mat;
mod minimap;
mod models;
mod notification;
mod operation;
mod pathing;
mod player;
mod rng;
mod rotator;
mod run;
mod services;
mod skill;
mod solvers;
mod task;
mod tracker;
mod utils;
```

- [ ] **Step 3: Run test to verify it fails (panics with `todo!()`)**

```powershell
cargo test -p backend new_returns_err_when_api_key_missing 2>&1
```
Expected: test panics or fails (the `todo!()` macro panics at runtime).

- [ ] **Step 4: Implement GeminiClient**

Replace the entire contents of `backend/src/gemini.rs` with:

```rust
use anyhow::{Result, bail};
use base64::{Engine as _, engine::general_purpose::STANDARD as BASE64};
use reqwest::blocking::Client;
use serde_json::json;

const GEMINI_URL: &str =
    "https://generativelanguage.googleapis.com/v1beta/models/gemini-2.0-flash:generateContent";

const PROMPT: &str =
    "Return only the exact characters visible in this image, with no spaces, \
     punctuation, or explanation. The text is alphanumeric and case-sensitive.";

pub struct GeminiClient {
    client: Client,
    api_key: String,
}

impl GeminiClient {
    pub fn new() -> Result<Self> {
        let api_key = std::env::var("GEMINI_API_KEY")
            .map_err(|_| anyhow::anyhow!("GEMINI_API_KEY environment variable not set"))?;
        Ok(Self {
            client: Client::new(),
            api_key,
        })
    }

    pub fn recognize(&self, png_bytes: &[u8]) -> Result<String> {
        let b64 = BASE64.encode(png_bytes);
        let url = format!("{}?key={}", GEMINI_URL, self.api_key);
        let body = json!({
            "contents": [{
                "parts": [
                    {
                        "inline_data": {
                            "mime_type": "image/png",
                            "data": b64
                        }
                    },
                    { "text": PROMPT }
                ]
            }]
        });

        let response = self
            .client
            .post(&url)
            .json(&body)
            .send()?;

        if !response.status().is_success() {
            bail!("Gemini API error: HTTP {}", response.status());
        }

        let json: serde_json::Value = response.json()?;
        let raw = json["candidates"][0]["content"]["parts"][0]["text"]
            .as_str()
            .ok_or_else(|| anyhow::anyhow!("unexpected Gemini response: {:?}", json))?;

        let cleaned: String = raw.trim().chars().filter(|c| c.is_alphanumeric()).collect();

        if cleaned.is_empty() {
            bail!("Gemini returned no alphanumeric text from: {:?}", raw);
        }
        Ok(cleaned)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn new_returns_err_when_api_key_missing() {
        std::env::remove_var("GEMINI_API_KEY");
        let result = GeminiClient::new();
        assert!(result.is_err(), "expected Err when GEMINI_API_KEY is not set");
    }
}
```

- [ ] **Step 5: Run test to verify it passes**

```powershell
cargo test -p backend new_returns_err_when_api_key_missing 2>&1
```
Expected: `test gemini::tests::new_returns_err_when_api_key_missing ... ok`

---

## Task 3: Replace ocr_captcha_region in detect.rs

**Files:**
- Modify: `backend/src/detect.rs`

The function is at approximately line 2469. The current body calls `extract_texts`. Replace the entire function body.

- [ ] **Step 1: Add gemini import to detect.rs**

Near the top of `backend/src/detect.rs`, find the existing `use crate::` block. Add `GeminiClient` to the imports:

```rust
use crate::{bridge::KeyKind, gemini::GeminiClient, models::Localization};
```

(The existing line is `use crate::{bridge::KeyKind, models::Localization};` — add `gemini::GeminiClient,` into it.)

- [ ] **Step 2: Replace ocr_captcha_region body**

Find the current function (around line 2468–2483):

```rust
/// Runs OCR on a pre-cropped captcha text region. Exposed for testing.
pub fn ocr_captcha_region(text_bgr: &impl MatTraitConst) -> Result<String> {
    // The captcha region is already pre-cropped to just the text band — CRAFT detection
    // is not needed and actually hurts accuracy on these small images because the captcha
    // font produces text_score < 0.7, causing characters to be silently dropped.
    // Feed the full region directly to the recognizer.
    let size = text_bgr.size()?;
    let full_bbox = Rect::new(0, 0, size.width, size.height);
    let text = extract_texts(text_bgr, &[full_bbox])
        .into_iter()
        .collect::<String>();
    if text.is_empty() {
        bail!("no text extracted from captcha region");
    }
    Ok(text)
}
```

Replace with:

```rust
/// Runs OCR on a pre-cropped captcha text region via the Gemini Flash API.
///
/// Requires the `GEMINI_API_KEY` environment variable to be set.
pub fn ocr_captcha_region(text_bgr: &impl MatTraitConst) -> Result<String> {
    let mut png_buf = opencv::core::Vector::<u8>::default();
    opencv::imgcodecs::imencode(
        ".png",
        text_bgr,
        &mut png_buf,
        &opencv::core::Vector::default(),
    )?;
    let client = GeminiClient::new()?;
    client.recognize(png_buf.as_slice())
}
```

- [ ] **Step 3: Verify it compiles**

```powershell
cargo check -p backend 2>&1
```
Expected: no errors. If you see "unused import" warnings for `extract_texts` or related symbols in the captcha section, that is expected and fine — those functions are still used by other detections.

- [ ] **Step 4: Run existing tests to verify nothing else broke**

```powershell
cargo test -p backend 2>&1
```
Expected: all existing tests pass (the captcha unit tests in `solvers/captcha.rs` test key parsing, not OCR — they will still pass).

---

## Task 4: Update test_captcha_ocr example

**Files:**
- Modify: `backend/examples/test_captcha_ocr.rs`

- [ ] **Step 1: Update the example to handle missing API key gracefully**

Replace the entire file content with:

```rust
use std::path::PathBuf;

use backend::detect::ocr_captcha_region;
use opencv::imgcodecs::{IMREAD_COLOR, imread};

fn main() {
    if std::env::var("GEMINI_API_KEY").is_err() {
        eprintln!("GEMINI_API_KEY is not set — skipping captcha OCR test");
        return;
    }

    let samples = [
        ("sample_1.png", "vtuutwezrZ"),
        ("sample_2.png", "xQthUPBrLT"),
        ("sample_3.png", "MQiiWSQFQo"),
        ("sample_4.png", "fGVstXcbrb"),
    ];

    let base = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("resources")
        .join("captcha_test");

    let mut all_passed = true;

    for (filename, expected) in &samples {
        let path = base.join(filename);
        let img = imread(path.to_str().unwrap(), IMREAD_COLOR).unwrap();

        match ocr_captcha_region(&img) {
            Ok(got) => {
                let pass = got == *expected;
                let status = if pass { "PASS" } else { "FAIL" };
                println!("[{status}] {filename}: got={got:?} expected={expected:?}");
                if !pass {
                    all_passed = false;
                }
            }
            Err(e) => {
                println!("[FAIL] {filename}: error: {e}");
                all_passed = false;
            }
        }
    }

    if !all_passed {
        std::process::exit(1);
    }
}
```

- [ ] **Step 2: Verify the example compiles**

```powershell
cargo build --example test_captcha_ocr -p backend 2>&1
```
Expected: compiles cleanly.

- [ ] **Step 3: Run the example with a real API key to verify end-to-end**

```powershell
$env:GEMINI_API_KEY = "your-key-here"
cargo run --example test_captcha_ocr -p backend 2>&1
```
Expected output (all 4 samples should pass):
```
[PASS] sample_1.png: got="vtuutwezrZ" expected="vtuutwezrZ"
[PASS] sample_2.png: got="xQthUPBrLT" expected="xQthUPBrLT"
[PASS] sample_3.png: got="MQiiWSQFQo" expected="MQiiWSQFQo"
[PASS] sample_4.png: got="fGVstXcbrb" expected="fGVstXcbrb"
```

If a sample fails, check the raw response by temporarily adding `dbg!` around the `recognize` call in `gemini.rs`.

---

## Done

At this point:
- `GEMINI_API_KEY` env var → bot solves captchas via Gemini 2.0 Flash
- Missing key → `ocr_captcha_region` returns `Err`, state machine retries (existing behaviour)
- All other OCR (player name, HP bar) unchanged — `extract_texts` and both ONNX models untouched
- No new model files, no training required
