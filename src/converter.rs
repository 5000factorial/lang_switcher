use crate::keymap::{Direction, Layout, convert_char, direction_from_layouts};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SelectedTextDecision {
    UsToRu,
    RuToUs,
}

impl SelectedTextDecision {
    pub fn as_direction(self) -> Direction {
        match self {
            Self::UsToRu => Direction::UsToRu,
            Self::RuToUs => Direction::RuToUs,
        }
    }

    pub fn target_layout(self) -> Layout {
        match self {
            Self::UsToRu => Layout::Ru,
            Self::RuToUs => Layout::Us,
        }
    }
}

pub fn convert_text(direction: Direction, text: &str) -> String {
    text.chars()
        .map(|ch| convert_char(direction, ch).unwrap_or(ch))
        .collect()
}

pub fn detect_selection_direction(text: &str, current: Layout) -> SelectedTextDecision {
    let mut latin = 0usize;
    let mut cyrillic = 0usize;

    for ch in text.chars() {
        if ch.is_ascii_alphabetic() {
            latin += 1;
        } else if ('а'..='я').contains(&ch) || ('А'..='Я').contains(&ch) || ch == 'ё' || ch == 'Ё'
        {
            cyrillic += 1;
        }
    }

    if cyrillic > latin {
        SelectedTextDecision::RuToUs
    } else if latin > cyrillic {
        SelectedTextDecision::UsToRu
    } else {
        match direction_from_layouts(current, opposite_layout(current)) {
            Direction::UsToRu => SelectedTextDecision::UsToRu,
            Direction::RuToUs => SelectedTextDecision::RuToUs,
        }
    }
}

fn opposite_layout(layout: Layout) -> Layout {
    match layout {
        Layout::Us => Layout::Ru,
        Layout::Ru => Layout::Us,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn converts_ru_to_us() {
        assert_eq!(convert_text(Direction::RuToUs, "руддщ"), "hello");
    }

    #[test]
    fn converts_us_to_ru() {
        assert_eq!(convert_text(Direction::UsToRu, "ghbdtn"), "привет");
    }

    #[test]
    fn keeps_punctuation_positions() {
        assert_eq!(convert_text(Direction::RuToUs, "руддщ!"), "hello!");
    }

    #[test]
    fn detects_script_bias() {
        assert_eq!(
            detect_selection_direction("привет", Layout::Us),
            SelectedTextDecision::RuToUs
        );
        assert_eq!(
            detect_selection_direction("hello", Layout::Ru),
            SelectedTextDecision::UsToRu
        );
    }
}
