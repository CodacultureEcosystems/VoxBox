//! Map ASCII text to Linux evdev keycodes for uinput-style injection.

pub const KEY_ESC: u16 = 1;
pub const KEY_1: u16 = 2;
pub const KEY_2: u16 = 3;
pub const KEY_3: u16 = 4;
pub const KEY_4: u16 = 5;
pub const KEY_5: u16 = 6;
pub const KEY_6: u16 = 7;
pub const KEY_7: u16 = 8;
pub const KEY_8: u16 = 9;
pub const KEY_9: u16 = 10;
pub const KEY_0: u16 = 11;
pub const KEY_MINUS: u16 = 12;
pub const KEY_EQUAL: u16 = 13;
pub const KEY_TAB: u16 = 15;
pub const KEY_LEFTBRACE: u16 = 26;
pub const KEY_RIGHTBRACE: u16 = 27;
pub const KEY_ENTER: u16 = 28;
pub const KEY_LEFTCTRL: u16 = 29;
pub const KEY_A: u16 = 30;
pub const KEY_S: u16 = 31;
pub const KEY_D: u16 = 32;
pub const KEY_F: u16 = 33;
pub const KEY_G: u16 = 34;
pub const KEY_H: u16 = 35;
pub const KEY_J: u16 = 36;
pub const KEY_K: u16 = 37;
pub const KEY_L: u16 = 38;
pub const KEY_SEMICOLON: u16 = 39;
pub const KEY_APOSTROPHE: u16 = 40;
pub const KEY_GRAVE: u16 = 41;
pub const KEY_LEFTSHIFT: u16 = 42;
pub const KEY_BACKSLASH: u16 = 43;
pub const KEY_Z: u16 = 44;
pub const KEY_X: u16 = 45;
pub const KEY_C: u16 = 46;
pub const KEY_V: u16 = 47;
pub const KEY_B: u16 = 48;
pub const KEY_N: u16 = 49;
pub const KEY_M: u16 = 50;
pub const KEY_COMMA: u16 = 51;
pub const KEY_DOT: u16 = 52;
pub const KEY_SLASH: u16 = 53;
pub const KEY_SPACE: u16 = 57;

/// A key press as (keycode, shift_required).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Keypress {
    pub code: u16,
    pub shift: bool,
}

/// Map one char to a keypress; None = not on a standard US layout.
pub fn char_to_keypress(c: char) -> Option<Keypress> {
    let qwerty = |ch: char| -> Option<u16> {
        Some(match ch {
            'q' => 16,
            'w' => 17,
            'e' => 18,
            'r' => 19,
            't' => 20,
            'y' => 21,
            'u' => 22,
            'i' => 23,
            'o' => 24,
            'p' => 25,
            'a' => 30,
            's' => 31,
            'd' => 32,
            'f' => 33,
            'g' => 34,
            'h' => 35,
            'j' => 36,
            'k' => 37,
            'l' => 38,
            'z' => 44,
            'x' => 45,
            'c' => 46,
            'v' => 47,
            'b' => 48,
            'n' => 49,
            'm' => 50,
            _ => return None,
        })
    };
    let k = match c {
        c if c.is_ascii_lowercase() => Keypress {
            code: qwerty(c)?,
            shift: false,
        },
        c if c.is_ascii_uppercase() => Keypress {
            code: qwerty(c.to_ascii_lowercase())?,
            shift: true,
        },
        '1'..='9' => Keypress { code: KEY_1 + (c as u16 - '1' as u16), shift: false },
        '0' => Keypress { code: KEY_0, shift: false },
        ' ' => Keypress { code: KEY_SPACE, shift: false },
        '\n' | '\r' => Keypress { code: KEY_ENTER, shift: false },
        '\t' => Keypress { code: KEY_TAB, shift: false },
        '-' => Keypress { code: KEY_MINUS, shift: false },
        '=' => Keypress { code: KEY_EQUAL, shift: false },
        '[' => Keypress { code: KEY_LEFTBRACE, shift: false },
        ']' => Keypress { code: KEY_RIGHTBRACE, shift: false },
        '\\' => Keypress { code: KEY_BACKSLASH, shift: false },
        ';' => Keypress { code: KEY_SEMICOLON, shift: false },
        '\'' => Keypress { code: KEY_APOSTROPHE, shift: false },
        '`' => Keypress { code: KEY_GRAVE, shift: false },
        ',' => Keypress { code: KEY_COMMA, shift: false },
        '.' => Keypress { code: KEY_DOT, shift: false },
        '/' => Keypress { code: KEY_SLASH, shift: false },
        '!' => Keypress { code: KEY_1, shift: true },
        '@' => Keypress { code: KEY_2, shift: true },
        '#' => Keypress { code: KEY_3, shift: true },
        '$' => Keypress { code: KEY_4, shift: true },
        '%' => Keypress { code: KEY_5, shift: true },
        '^' => Keypress { code: KEY_6, shift: true },
        '&' => Keypress { code: KEY_7, shift: true },
        '*' => Keypress { code: KEY_8, shift: true },
        '(' => Keypress { code: KEY_9, shift: true },
        ')' => Keypress { code: KEY_0, shift: true },
        '_' => Keypress { code: KEY_MINUS, shift: true },
        '+' => Keypress { code: KEY_EQUAL, shift: true },
        '{' => Keypress { code: KEY_LEFTBRACE, shift: true },
        '}' => Keypress { code: KEY_RIGHTBRACE, shift: true },
        '|' => Keypress { code: KEY_BACKSLASH, shift: true },
        ':' => Keypress { code: KEY_SEMICOLON, shift: true },
        '"' => Keypress { code: KEY_APOSTROPHE, shift: true },
        '~' => Keypress { code: KEY_GRAVE, shift: true },
        '<' => Keypress { code: KEY_COMMA, shift: true },
        '>' => Keypress { code: KEY_DOT, shift: true },
        '?' => Keypress { code: KEY_SLASH, shift: true },
        _ => return None,
    };
    Some(k)
}

/// True when every char in the text is typeable on a standard US layout.
pub fn ascii_typeable(text: &str) -> bool {
    text.chars().all(|c| char_to_keypress(c).is_some())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn lowercase_maps_without_shift() {
        assert_eq!(char_to_keypress('a'), Some(Keypress { code: KEY_A, shift: false }));
        assert_eq!(char_to_keypress('z'), Some(Keypress { code: KEY_Z, shift: false }));
    }

    #[test]
    fn uppercase_maps_with_shift() {
        assert_eq!(char_to_keypress('A'), Some(Keypress { code: KEY_A, shift: true }));
        assert_eq!(char_to_keypress('Q'), Some(Keypress { code: 16, shift: true }));
        assert_eq!(char_to_keypress('Z'), Some(Keypress { code: 44, shift: true }));
    }

    #[test]
    fn digits_and_space() {
        assert_eq!(char_to_keypress('5'), Some(Keypress { code: KEY_5, shift: false }));
        assert_eq!(char_to_keypress('0'), Some(Keypress { code: KEY_0, shift: false }));
        assert_eq!(char_to_keypress(' '), Some(Keypress { code: KEY_SPACE, shift: false }));
    }

    #[test]
    fn shifted_punctuation() {
        assert_eq!(char_to_keypress('!'), Some(Keypress { code: KEY_1, shift: true }));
        assert_eq!(char_to_keypress('?'), Some(Keypress { code: KEY_SLASH, shift: true }));
        assert_eq!(char_to_keypress(':'), Some(Keypress { code: KEY_SEMICOLON, shift: true }));
        assert_eq!(char_to_keypress('"'), Some(Keypress { code: KEY_APOSTROPHE, shift: true }));
        assert_eq!(char_to_keypress('('), Some(Keypress { code: KEY_9, shift: true }));
    }

    #[test]
    fn newline_is_enter_tab_is_tab() {
        assert_eq!(char_to_keypress('\n'), Some(Keypress { code: KEY_ENTER, shift: false }));
        assert_eq!(char_to_keypress('\t'), Some(Keypress { code: KEY_TAB, shift: false }));
    }

    #[test]
    fn unicode_not_typeable() {
        assert_eq!(char_to_keypress('é'), None);
        assert_eq!(char_to_keypress('😀'), None);
        assert!(!ascii_typeable("café"));
        assert!(ascii_typeable("Hello, world! (2+2=4)"));
    }
}
