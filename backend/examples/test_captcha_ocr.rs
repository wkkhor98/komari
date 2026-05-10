use std::path::PathBuf;

use backend::ocr_captcha_region;
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
