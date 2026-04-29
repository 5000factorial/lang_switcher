use std::collections::HashMap;
use std::sync::LazyLock;

use evdev::KeyCode;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Layout {
    Us,
    Ru,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Direction {
    UsToRu,
    RuToUs,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct KeyStrokeSpec {
    pub code: KeyCode,
    pub shifted: bool,
}

#[derive(Debug, Clone, Copy)]
pub struct KeyEntry {
    pub code: KeyCode,
    pub us: [Option<char>; 2],
    pub ru: [Option<char>; 2],
}

impl KeyEntry {
    pub const fn new(code: KeyCode, us0: char, us1: char, ru0: char, ru1: char) -> Self {
        Self {
            code,
            us: [Some(us0), Some(us1)],
            ru: [Some(ru0), Some(ru1)],
        }
    }

    pub const fn letter(code: KeyCode, us0: char, us1: char, ru0: char, ru1: char) -> Self {
        Self::new(code, us0, us1, ru0, ru1)
    }

    pub fn char_for(&self, layout: Layout, shifted: bool) -> Option<char> {
        let idx = usize::from(shifted);
        match layout {
            Layout::Us => self.us[idx],
            Layout::Ru => self.ru[idx],
        }
    }
}

pub const KEYMAP: &[KeyEntry] = &[
    KeyEntry::new(KeyCode::KEY_GRAVE, '`', '~', 'ё', 'Ё'),
    KeyEntry::new(KeyCode::KEY_1, '1', '!', '1', '!'),
    KeyEntry::new(KeyCode::KEY_2, '2', '@', '2', '"'),
    KeyEntry::new(KeyCode::KEY_3, '3', '#', '3', '№'),
    KeyEntry::new(KeyCode::KEY_4, '4', '$', '4', ';'),
    KeyEntry::new(KeyCode::KEY_5, '5', '%', '5', '%'),
    KeyEntry::new(KeyCode::KEY_6, '6', '^', '6', ':'),
    KeyEntry::new(KeyCode::KEY_7, '7', '&', '7', '?'),
    KeyEntry::new(KeyCode::KEY_8, '8', '*', '8', '*'),
    KeyEntry::new(KeyCode::KEY_9, '9', '(', '9', '('),
    KeyEntry::new(KeyCode::KEY_0, '0', ')', '0', ')'),
    KeyEntry::new(KeyCode::KEY_MINUS, '-', '_', '-', '_'),
    KeyEntry::new(KeyCode::KEY_EQUAL, '=', '+', '=', '+'),
    KeyEntry::letter(KeyCode::KEY_Q, 'q', 'Q', 'й', 'Й'),
    KeyEntry::letter(KeyCode::KEY_W, 'w', 'W', 'ц', 'Ц'),
    KeyEntry::letter(KeyCode::KEY_E, 'e', 'E', 'у', 'У'),
    KeyEntry::letter(KeyCode::KEY_R, 'r', 'R', 'к', 'К'),
    KeyEntry::letter(KeyCode::KEY_T, 't', 'T', 'е', 'Е'),
    KeyEntry::letter(KeyCode::KEY_Y, 'y', 'Y', 'н', 'Н'),
    KeyEntry::letter(KeyCode::KEY_U, 'u', 'U', 'г', 'Г'),
    KeyEntry::letter(KeyCode::KEY_I, 'i', 'I', 'ш', 'Ш'),
    KeyEntry::letter(KeyCode::KEY_O, 'o', 'O', 'щ', 'Щ'),
    KeyEntry::letter(KeyCode::KEY_P, 'p', 'P', 'з', 'З'),
    KeyEntry::letter(KeyCode::KEY_LEFTBRACE, '[', '{', 'х', 'Х'),
    KeyEntry::letter(KeyCode::KEY_RIGHTBRACE, ']', '}', 'ъ', 'Ъ'),
    KeyEntry::letter(KeyCode::KEY_A, 'a', 'A', 'ф', 'Ф'),
    KeyEntry::letter(KeyCode::KEY_S, 's', 'S', 'ы', 'Ы'),
    KeyEntry::letter(KeyCode::KEY_D, 'd', 'D', 'в', 'В'),
    KeyEntry::letter(KeyCode::KEY_F, 'f', 'F', 'а', 'А'),
    KeyEntry::letter(KeyCode::KEY_G, 'g', 'G', 'п', 'П'),
    KeyEntry::letter(KeyCode::KEY_H, 'h', 'H', 'р', 'Р'),
    KeyEntry::letter(KeyCode::KEY_J, 'j', 'J', 'о', 'О'),
    KeyEntry::letter(KeyCode::KEY_K, 'k', 'K', 'л', 'Л'),
    KeyEntry::letter(KeyCode::KEY_L, 'l', 'L', 'д', 'Д'),
    KeyEntry::letter(KeyCode::KEY_SEMICOLON, ';', ':', 'ж', 'Ж'),
    KeyEntry::letter(KeyCode::KEY_APOSTROPHE, '\'', '"', 'э', 'Э'),
    KeyEntry::new(KeyCode::KEY_BACKSLASH, '\\', '|', '\\', '/'),
    KeyEntry::letter(KeyCode::KEY_Z, 'z', 'Z', 'я', 'Я'),
    KeyEntry::letter(KeyCode::KEY_X, 'x', 'X', 'ч', 'Ч'),
    KeyEntry::letter(KeyCode::KEY_C, 'c', 'C', 'с', 'С'),
    KeyEntry::letter(KeyCode::KEY_V, 'v', 'V', 'м', 'М'),
    KeyEntry::letter(KeyCode::KEY_B, 'b', 'B', 'и', 'И'),
    KeyEntry::letter(KeyCode::KEY_N, 'n', 'N', 'т', 'Т'),
    KeyEntry::letter(KeyCode::KEY_M, 'm', 'M', 'ь', 'Ь'),
    KeyEntry::letter(KeyCode::KEY_COMMA, ',', '<', 'б', 'Б'),
    KeyEntry::letter(KeyCode::KEY_DOT, '.', '>', 'ю', 'Ю'),
    KeyEntry::new(KeyCode::KEY_SLASH, '/', '?', '.', ','),
    KeyEntry::new(KeyCode::KEY_SPACE, ' ', ' ', ' ', ' '),
];

static US_REVERSE: LazyLock<HashMap<char, KeyStrokeSpec>> =
    LazyLock::new(|| build_reverse(Layout::Us));
static RU_REVERSE: LazyLock<HashMap<char, KeyStrokeSpec>> =
    LazyLock::new(|| build_reverse(Layout::Ru));

pub fn parse_layout(name: &str) -> Option<Layout> {
    let normalized = name.trim().to_ascii_lowercase();
    for token in normalized.split(|ch: char| !ch.is_ascii_alphanumeric()) {
        match token {
            "us" | "en" | "eng" => return Some(Layout::Us),
            "ru" | "rus" => return Some(Layout::Ru),
            _ => {}
        }
    }
    None
}

pub fn direction_from_layouts(source: Layout, target: Layout) -> Direction {
    match (source, target) {
        (Layout::Us, Layout::Ru) => Direction::UsToRu,
        (Layout::Ru, Layout::Us) => Direction::RuToUs,
        (Layout::Us, Layout::Us) => Direction::UsToRu,
        (Layout::Ru, Layout::Ru) => Direction::RuToUs,
    }
}

pub fn key_to_char(code: KeyCode, layout: Layout, shifted: bool) -> Option<char> {
    KEYMAP
        .iter()
        .find(|entry| entry.code == code)
        .and_then(|entry| entry.char_for(layout, shifted))
}

pub fn char_to_key(layout: Layout, ch: char) -> Option<KeyStrokeSpec> {
    match layout {
        Layout::Us => US_REVERSE.get(&ch).copied(),
        Layout::Ru => RU_REVERSE.get(&ch).copied(),
    }
}

pub fn convert_char(direction: Direction, ch: char) -> Option<char> {
    let (source, target) = match direction {
        Direction::UsToRu => (Layout::Us, Layout::Ru),
        Direction::RuToUs => (Layout::Ru, Layout::Us),
    };
    let key = char_to_key(source, ch)?;
    key_to_char(key.code, target, key.shifted)
}

pub fn is_word_char(ch: char) -> bool {
    ch.is_ascii_alphanumeric()
        || ('а'..='я').contains(&ch)
        || ('А'..='Я').contains(&ch)
        || ch == 'ё'
        || ch == 'Ё'
}

pub fn is_delimiter(ch: char) -> bool {
    ch.is_whitespace() || (!is_word_char(ch) && ch != '\0')
}

fn build_reverse(layout: Layout) -> HashMap<char, KeyStrokeSpec> {
    let mut map = HashMap::new();
    for entry in KEYMAP {
        for shifted in [false, true] {
            if let Some(ch) = entry.char_for(layout, shifted) {
                map.insert(
                    ch,
                    KeyStrokeSpec {
                        code: entry.code,
                        shifted,
                    },
                );
            }
        }
    }
    map
}

#[cfg(test)]
mod tests {
    use super::{Layout, parse_layout};

    #[test]
    fn parses_plain_layout_names() {
        assert_eq!(parse_layout("us"), Some(Layout::Us));
        assert_eq!(parse_layout("ru"), Some(Layout::Ru));
    }

    #[test]
    fn parses_variant_layout_names() {
        assert_eq!(parse_layout("us+altgr-intl"), Some(Layout::Us));
        assert_eq!(parse_layout("ru(phonetic)"), Some(Layout::Ru));
        assert_eq!(parse_layout("xkb:us::eng"), Some(Layout::Us));
        assert_eq!(parse_layout("xkb:ru::rus"), Some(Layout::Ru));
    }
}
