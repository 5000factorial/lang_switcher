use std::time::Duration;

use anyhow::Result;

use crate::atspi_bridge::{AtspiBridge, SelectionConversion};
use crate::converter::{convert_text, detect_selection_direction};
use crate::keymap::Layout;
use crate::primary_selection;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SelectionOutcome {
    Handled {
        target_layout: Layout,
        replacement_text: String,
    },
    NoSelection,
    Unsupported,
}

impl SelectionOutcome {
    pub fn target_layout(self) -> Option<Layout> {
        match self {
            Self::Handled { target_layout, .. } => Some(target_layout),
            Self::NoSelection | Self::Unsupported => None,
        }
    }
}

pub async fn try_handle_selection(
    atspi: Option<&AtspiBridge>,
    current_layout: Layout,
) -> Result<SelectionOutcome> {
    let Some(atspi) = atspi else {
        return Ok(SelectionOutcome::Unsupported);
    };

    match atspi.try_convert_selection(current_layout).await? {
        Some(conversion) => {
            atspi.clear_recent_text_selection().await;
            Ok(outcome_from_conversion(conversion))
        }
        None => try_primary_selection_fallback(atspi, current_layout).await,
    }
}

pub fn should_fallback_to_last_word(result: &Result<SelectionOutcome>) -> bool {
    !matches!(result, Ok(SelectionOutcome::Handled { .. }))
}

pub fn configured_mode(enabled: bool) -> &'static str {
    if enabled {
        "atspi + primary-selection (best-effort)"
    } else {
        "disabled"
    }
}

fn outcome_from_conversion(conversion: SelectionConversion) -> SelectionOutcome {
    SelectionOutcome::Handled {
        target_layout: conversion.decision.target_layout(),
        replacement_text: conversion.converted_text,
    }
}

async fn try_primary_selection_fallback(
    atspi: &AtspiBridge,
    current_layout: Layout,
) -> Result<SelectionOutcome> {
    if !atspi.saw_recent_text_selection().await {
        return Ok(SelectionOutcome::NoSelection);
    }

    let Some(selected) = primary_selection::read(Duration::from_millis(200)).await? else {
        return Ok(SelectionOutcome::NoSelection);
    };
    if selected.trim().is_empty() {
        return Ok(SelectionOutcome::NoSelection);
    }

    let decision = detect_selection_direction(&selected, current_layout);
    let converted = convert_text(decision.as_direction(), &selected);
    if converted == selected {
        return Ok(SelectionOutcome::NoSelection);
    }

    let target_layout = decision.target_layout();
    if !primary_selection::supports_injection(target_layout, &converted) {
        return Ok(SelectionOutcome::Unsupported);
    }
    atspi.clear_recent_text_selection().await;
    Ok(SelectionOutcome::Handled {
        target_layout,
        replacement_text: converted,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::converter::SelectedTextDecision;
    use anyhow::anyhow;

    #[test]
    fn handled_selection_skips_last_word_fallback() {
        let result = Ok(SelectionOutcome::Handled {
            target_layout: Layout::Us,
            replacement_text: "hello".to_owned(),
        });
        assert!(!should_fallback_to_last_word(&result));
    }

    #[test]
    fn missing_selection_uses_last_word_fallback() {
        let result = Ok(SelectionOutcome::NoSelection);
        assert!(should_fallback_to_last_word(&result));
    }

    #[test]
    fn unsupported_selection_uses_last_word_fallback() {
        let result = Ok(SelectionOutcome::Unsupported);
        assert!(should_fallback_to_last_word(&result));
    }

    #[test]
    fn selection_errors_still_use_last_word_fallback() {
        let result: Result<SelectionOutcome> = Err(anyhow!("boom"));
        assert!(should_fallback_to_last_word(&result));
    }

    #[test]
    fn maps_conversion_to_target_layout() {
        let result = outcome_from_conversion(SelectionConversion {
            decision: SelectedTextDecision::RuToUs,
            converted_text: String::new(),
        });
        assert_eq!(
            result,
            SelectionOutcome::Handled {
                target_layout: Layout::Us,
                replacement_text: String::new()
            }
        );
    }

    #[test]
    fn reports_configured_mode() {
        assert_eq!(
            configured_mode(true),
            "atspi + primary-selection (best-effort)"
        );
        assert_eq!(configured_mode(false), "disabled");
    }
}
