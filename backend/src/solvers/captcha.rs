use log::warn;

use crate::bridge::KeyKind;

/// Parses a string of captcha characters into a sequence of (needs_shift, KeyKind) pairs.
pub fn parse_captcha_chars(text: &str) -> Vec<(bool, KeyKind)> {
    text.chars().filter_map(char_to_key).collect()
}

fn char_to_key(c: char) -> Option<(bool, KeyKind)> {
    let needs_shift = c.is_ascii_uppercase();
    let key = match c.to_ascii_lowercase() {
        'a' => KeyKind::A,
        'b' => KeyKind::B,
        'c' => KeyKind::C,
        'd' => KeyKind::D,
        'e' => KeyKind::E,
        'f' => KeyKind::F,
        'g' => KeyKind::G,
        'h' => KeyKind::H,
        'i' => KeyKind::I,
        'j' => KeyKind::J,
        'k' => KeyKind::K,
        'l' => KeyKind::L,
        'm' => KeyKind::M,
        'n' => KeyKind::N,
        'o' => KeyKind::O,
        'p' => KeyKind::P,
        'q' => KeyKind::Q,
        'r' => KeyKind::R,
        's' => KeyKind::S,
        't' => KeyKind::T,
        'u' => KeyKind::U,
        'v' => KeyKind::V,
        'w' => KeyKind::W,
        'x' => KeyKind::X,
        'y' => KeyKind::Y,
        'z' => KeyKind::Z,
        '0' => KeyKind::Zero,
        '1' => KeyKind::One,
        '2' => KeyKind::Two,
        '3' => KeyKind::Three,
        '4' => KeyKind::Four,
        '5' => KeyKind::Five,
        '6' => KeyKind::Six,
        '7' => KeyKind::Seven,
        '8' => KeyKind::Eight,
        '9' => KeyKind::Nine,
        other => {
            warn!(target: "backend/player", "unknown captcha char: {other}");
            return None;
        }
    };
    Some((needs_shift, key))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_lowercase_letters() {
        let result = parse_captcha_chars("abc");
        assert_eq!(
            result,
            vec![
                (false, KeyKind::A),
                (false, KeyKind::B),
                (false, KeyKind::C),
            ]
        );
    }

    #[test]
    fn test_uppercase_letters() {
        let result = parse_captcha_chars("ABC");
        assert_eq!(
            result,
            vec![
                (true, KeyKind::A),
                (true, KeyKind::B),
                (true, KeyKind::C),
            ]
        );
    }

    #[test]
    fn test_digits() {
        let result = parse_captcha_chars("019");
        assert_eq!(
            result,
            vec![
                (false, KeyKind::Zero),
                (false, KeyKind::One),
                (false, KeyKind::Nine),
            ]
        );
    }

    #[test]
    fn test_mixed_case_example() {
        // "KsAXcwvgUQ" from the screenshot
        let result = parse_captcha_chars("KsAXcwvgUQ");
        assert_eq!(result.len(), 10);
        assert_eq!(result[0], (true, KeyKind::K)); // K uppercase
        assert_eq!(result[1], (false, KeyKind::S)); // s lowercase
        assert_eq!(result[2], (true, KeyKind::A)); // A uppercase
        assert_eq!(result[3], (true, KeyKind::X)); // X uppercase
        assert_eq!(result[4], (false, KeyKind::C)); // c lowercase
        assert_eq!(result[9], (true, KeyKind::Q)); // Q uppercase
    }

    #[test]
    fn test_unknown_chars_are_skipped() {
        let result = parse_captcha_chars("a!b@c");
        assert_eq!(
            result,
            vec![
                (false, KeyKind::A),
                (false, KeyKind::B),
                (false, KeyKind::C),
            ]
        );
    }

    #[test]
    fn test_empty_string() {
        assert!(parse_captcha_chars("").is_empty());
    }
}
