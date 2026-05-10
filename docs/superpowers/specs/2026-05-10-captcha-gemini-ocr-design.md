# Captcha OCR — Replace ONNX Recognizer with Gemini Flash API

**Date:** 2026-05-10  
**Status:** Approved

## Problem

The existing captcha OCR pipeline feeds a pre-cropped 250×25 px text strip directly into a CRNN-based ONNX recognizer (`text_recognition.onnx`). After removing the CRAFT detection step (which was silently dropping characters), the recognizer now produces completely wrong output. Multiple attempts to tune the model and preprocessing have not resolved the issue. The decision is to drop the ONNX-based recognizer for captcha and replace it with a cloud vision API.

## Goal

Replace only `ocr_captcha_region` in `backend/src/detect.rs` with a call to the **Google Gemini 2.0 Flash** API. Everything else in the captcha pipeline stays unchanged: the state machine in `solve_captcha.rs`, the dialog/image detection, the retry logic, and the existing ONNX models used for other OCR tasks (player name, HP bar, etc.).

## Architecture

### What changes

| Location | Change |
|---|---|
| `backend/src/detect.rs` — `ocr_captcha_region` | Replace `extract_texts` call with Gemini API call |
| `backend/src/services/gemini.rs` (new) | `GeminiClient` struct: API key, HTTP client, image encode + POST |
| `backend/src/services/mod.rs` | Export `gemini` module |
| `backend/Cargo.toml` | Add `blocking` and `json` features to existing `reqwest` dependency |

### What stays unchanged

- `text_detection.onnx` and `text_recognition.onnx` — still used for player name, HP bar, and other detections
- `extract_texts` and `extract_text_bboxes` functions
- All of `solve_captcha.rs`
- `detect_lie_detector_captcha_text` — still crops the 250×25 ROI, then calls `ocr_captcha_region`
- Error handling contract: `ocr_captcha_region` returns `Result<String>`, same as before; failures cause the state machine to retry via `WaitingForImage`

### Data flow

```
cropped Mat (250×25 BGR)          [detect.rs: ocr_captcha_region]
  → opencv imencode → PNG bytes
  → base64::encode
  → GeminiClient::recognize(png_bytes)   [services/gemini.rs]
      → POST https://generativelanguage.googleapis.com/v1beta/models/
             gemini-2.0-flash:generateContent?key={GEMINI_API_KEY}
        body: { contents: [{ parts: [{ inline_data: image/png + base64 },
                                     { text: prompt }] }] }
      → parse response JSON → candidates[0].content.parts[0].text
      → strip whitespace
  → validate: non-empty, alphanumeric only
  → return Ok(String) or Err
```

### Prompt

```
Return only the exact characters visible in this image, with no spaces, punctuation, or explanation. The text is alphanumeric and case-sensitive.
```

### API key

Read from the `GEMINI_API_KEY` environment variable at runtime inside `GeminiClient::new()`. If the variable is missing, `ocr_captcha_region` returns an `Err` immediately, which the existing state machine handles as an OCR failure (notification + retry).

### Response validation

After receiving the response text:
1. Strip leading/trailing whitespace
2. If empty → return `Err`
3. Filter to only alphanumeric characters (drop any stray punctuation the model may add)
4. If still empty → return `Err`
5. Return the cleaned string

No length check — if the game ever changes the captcha length, the validation still works correctly.

## HTTP client

`GeminiClient` holds a `reqwest::blocking::Client` (the rest of the codebase uses blocking OpenCV calls, so blocking HTTP fits the existing threading model). The client is constructed once and reused.

The call is made synchronously from within `update_settling` after the settle timeout — same thread, same pattern as the existing ONNX inference calls.

## Error handling

All errors propagate as `anyhow::Error` matching the existing `Result<String>` return type of `ocr_captcha_region`. Network errors, JSON parse errors, empty responses, and missing API key all produce an `Err`. The state machine in `solve_captcha.rs` already handles OCR errors by logging a warning, sending a notification, and retrying from `WaitingForImage`.

## Testing

- Update `backend/examples/test_captcha_ocr.rs` to skip or document that it requires `GEMINI_API_KEY` set at runtime
- The 4 existing sample images (`sample_1.png` → `vtuutwezrZ`, etc.) remain as ground truth for manual verification

## Cost

Gemini 2.0 Flash free tier: **1,500 requests/day**. A captcha appears at most a handful of times per gaming session. Cost is effectively zero.
