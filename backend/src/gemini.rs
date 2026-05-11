use std::sync::Mutex;

use anyhow::{Result, bail};
use base64::{Engine as _, engine::general_purpose::STANDARD as BASE64};
use reqwest::blocking::Client;
use serde_json::json;

const GEMINI_URL: &str =
    "https://generativelanguage.googleapis.com/v1beta/models/gemini-3-flash-preview:generateContent";

const PROMPT: &str = "Return only the exact characters visible in this image, with no spaces, \
     punctuation, or explanation. The text is alphanumeric and case-sensitive.";

static SETTINGS_API_KEY: Mutex<Option<String>> = Mutex::new(None);

/// Sets the Gemini API key from application settings.
/// Takes priority over the `GEMINI_API_KEY` environment variable.
pub fn set_api_key(key: Option<String>) {
    *SETTINGS_API_KEY.lock().unwrap() = key;
}

pub struct GeminiClient {
    client: Client,
    api_key: String,
}

impl GeminiClient {
    pub fn new() -> Result<Self> {
        let api_key = SETTINGS_API_KEY
            .lock()
            .unwrap()
            .clone()
            .or_else(|| std::env::var("GEMINI_API_KEY").ok())
            .ok_or_else(|| {
                anyhow::anyhow!(
                    "Gemini API key not configured. Set it in Settings or via GEMINI_API_KEY env var."
                )
            })?;
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

        let response = self.client.post(&url).json(&body).send()?;

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
    fn new_returns_err_when_no_key_configured() {
        // SAFETY: single-threaded test environment, no concurrent env reads
        unsafe { std::env::remove_var("GEMINI_API_KEY") };
        set_api_key(None);
        let result = GeminiClient::new();
        assert!(
            result.is_err(),
            "expected Err when no API key is configured"
        );
    }

    #[test]
    fn new_succeeds_when_settings_key_set() {
        set_api_key(Some("test-key-from-settings".to_string()));
        let result = GeminiClient::new();
        assert!(result.is_ok(), "expected Ok when settings key is set");
        set_api_key(None); // cleanup
    }
}
