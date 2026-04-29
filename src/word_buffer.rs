use std::collections::VecDeque;

use evdev::KeyCode;

use crate::converter::convert_text;
use crate::keymap::{Direction, Layout, is_delimiter, is_word_char, key_to_char};

#[derive(Debug, Clone, Copy)]
pub struct BufferedKey {
    pub code: KeyCode,
    pub shifted: bool,
}

#[derive(Debug, Clone)]
pub enum BufferToken {
    Char(BufferedKey),
    Literal(char),
    Break,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ConversionPlan {
    pub delete_count: usize,
    pub replacement_text: String,
}

#[derive(Debug)]
pub struct WordBuffer {
    limit: usize,
    tokens: VecDeque<BufferToken>,
}

impl WordBuffer {
    pub fn new(limit: usize) -> Self {
        Self {
            limit,
            tokens: VecDeque::with_capacity(limit),
        }
    }

    pub fn push_char(&mut self, code: KeyCode, shifted: bool) {
        self.push(BufferToken::Char(BufferedKey { code, shifted }));
    }

    pub fn push_literal(&mut self, ch: char) {
        self.push(BufferToken::Literal(ch));
    }

    pub fn push_break(&mut self) {
        self.push(BufferToken::Break);
    }

    pub fn pop_last_char(&mut self) {
        self.tokens.pop_back();
    }

    pub fn clear(&mut self) {
        self.tokens.clear();
    }

    pub fn plan_conversion(
        &self,
        source_layout: Layout,
        direction: Direction,
    ) -> Option<ConversionPlan> {
        let chars: Vec<Option<char>> = self
            .tokens
            .iter()
            .map(|token| match token {
                BufferToken::Char(key) => key_to_char(key.code, source_layout, key.shifted),
                BufferToken::Literal(ch) => Some(*ch),
                BufferToken::Break => None,
            })
            .collect();

        if chars.is_empty() {
            return None;
        }

        let mut idx = chars.len();
        let mut suffix = String::new();

        while idx > 0 {
            match chars[idx - 1] {
                Some(ch) if is_delimiter(ch) => {
                    suffix.insert(0, ch);
                    idx -= 1;
                }
                _ => break,
            }
        }

        let end = idx;
        let mut word = String::new();
        while idx > 0 {
            match chars[idx - 1] {
                Some(ch) if is_word_char(ch) => {
                    word.insert(0, ch);
                    idx -= 1;
                }
                _ => break,
            }
        }

        if word.is_empty() {
            return None;
        }

        let replacement = convert_text(direction, &word);
        let delete_count = end - idx + suffix.chars().count();
        Some(ConversionPlan {
            delete_count,
            replacement_text: format!("{replacement}{suffix}"),
        })
    }

    fn push(&mut self, token: BufferToken) {
        if self.tokens.len() == self.limit {
            self.tokens.pop_front();
        }
        self.tokens.push_back(token);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::keymap::Direction;

    #[test]
    fn converts_last_word_without_suffix() {
        let mut buffer = WordBuffer::new(16);
        for code in [
            KeyCode::KEY_H,
            KeyCode::KEY_E,
            KeyCode::KEY_L,
            KeyCode::KEY_L,
            KeyCode::KEY_O,
        ] {
            buffer.push_char(code, false);
        }
        let plan = buffer
            .plan_conversion(Layout::Ru, Direction::RuToUs)
            .unwrap();
        assert_eq!(plan.delete_count, 5);
        assert_eq!(plan.replacement_text, "hello");
    }

    #[test]
    fn converts_last_word_with_trailing_space() {
        let mut buffer = WordBuffer::new(16);
        for code in [
            KeyCode::KEY_H,
            KeyCode::KEY_E,
            KeyCode::KEY_L,
            KeyCode::KEY_L,
            KeyCode::KEY_O,
        ] {
            buffer.push_char(code, false);
        }
        buffer.push_literal(' ');
        let plan = buffer
            .plan_conversion(Layout::Ru, Direction::RuToUs)
            .unwrap();
        assert_eq!(plan.delete_count, 6);
        assert_eq!(plan.replacement_text, "hello ");
    }
}
