use std::time::Duration;

use anyhow::{Context, Result};
use tokio::process::Command;

const MAX_SELECTION_CHARS: usize = 2048;
const MAX_SELECTION_LINES: usize = 16;

pub async fn read(timeout: Duration) -> Result<Option<String>> {
    let command = Command::new("wl-paste")
        .args(["--primary", "--no-newline"])
        .output();

    let output = match tokio::time::timeout(timeout, command).await {
        Ok(output) => output.context("failed to execute wl-paste")?,
        Err(_) => return Ok(None),
    };

    if !output.status.success() {
        return Ok(None);
    }

    let text = String::from_utf8(output.stdout).context("wl-paste returned non-UTF-8 data")?;
    if text.is_empty() {
        return Ok(None);
    }

    Ok(Some(text))
}

pub fn looks_like_live_selection(text: &str) -> bool {
    let trimmed = text.trim();
    if trimmed.is_empty() {
        return false;
    }

    if text.chars().count() > MAX_SELECTION_CHARS {
        return false;
    }

    text.lines().take(MAX_SELECTION_LINES + 1).count() <= MAX_SELECTION_LINES
}

pub fn supports_injection(layout: crate::keymap::Layout, text: &str) -> bool {
    text.chars()
        .all(|ch| matches!(ch, '\n' | '\t') || crate::keymap::char_to_key(layout, ch).is_some())
}

#[cfg(test)]
mod tests {
    use super::{looks_like_live_selection, supports_injection};
    use crate::keymap::Layout;

    #[test]
    fn supports_ascii() {
        assert!(supports_injection(Layout::Us, "hello world"));
    }

    #[test]
    fn supports_cyrillic() {
        assert!(supports_injection(Layout::Ru, "привет\nмир"));
    }

    #[test]
    fn rejects_unsupported_unicode() {
        assert!(!supports_injection(Layout::Us, "hello🙂"));
    }

    #[test]
    fn rejects_empty_selection_text() {
        assert!(!looks_like_live_selection("   "));
    }

    #[test]
    fn rejects_too_many_lines() {
        let text = (0..17).map(|_| "line").collect::<Vec<_>>().join("\n");
        assert!(!looks_like_live_selection(&text));
    }
}
