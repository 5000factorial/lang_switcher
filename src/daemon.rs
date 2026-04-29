use std::time::{Duration, Instant};

use anyhow::Result;
use evdev::KeyCode;
use tokio::sync::mpsc;
use tracing::{info, warn};

use crate::atspi_bridge::AtspiBridge;
use crate::config::AppConfig;
use crate::gnome::InputSourceManager;
use crate::hotkey::DoubleShiftDetector;
use crate::injector::Injector;
use crate::input_capture::KeyEvent;
use crate::keymap::{Direction, Layout};
use crate::selection::{self, SelectionOutcome};
use crate::word_buffer::WordBuffer;

pub async fn run(config: AppConfig) -> Result<()> {
    info!("starting lang-switcher daemon");

    let input_sources = InputSourceManager::new(config.layout_pair.clone());
    let mut injector = create_injector_with_retry().await?;
    let atspi = if config.enable_selected_text {
        match AtspiBridge::new().await {
            Ok(bridge) => Some(bridge),
            Err(error) => {
                warn!("AT-SPI bridge unavailable, selected-text conversion disabled: {error:#}");
                None
            }
        }
    } else {
        None
    };

    let (tx, mut rx) = mpsc::unbounded_channel();
    crate::input_capture::spawn(tx);

    let mut buffer = WordBuffer::new(config.buffer_len);
    let mut detector =
        DoubleShiftDetector::new(config.double_shift_timeout_ms, config.max_shift_hold_ms);
    let mut modifiers = ModifierState::default();

    loop {
        tokio::select! {
            _ = tokio::signal::ctrl_c() => {
                info!("received shutdown signal");
                return Ok(());
            }
            Some(event) = rx.recv() => {
                if handle_key_event(
                    &config,
                    event,
                    &mut modifiers,
                    &mut detector,
                    &mut buffer,
                    &input_sources,
                    atspi.as_ref(),
                    &mut injector,
                ).await? {
                    buffer.clear();
                }
            }
        }
    }
}

async fn create_injector_with_retry() -> Result<Injector> {
    const ATTEMPTS: usize = 10;
    const DELAY_MS: u64 = 500;

    let mut last_error = None;
    for attempt in 1..=ATTEMPTS {
        match Injector::new() {
            Ok(injector) => {
                if attempt > 1 {
                    info!("uinput became available on attempt {attempt}");
                }
                return Ok(injector);
            }
            Err(error) => {
                last_error = Some(error);
                warn!(
                    "failed to initialize uinput on attempt {attempt}/{ATTEMPTS}; retrying in {} ms",
                    DELAY_MS
                );
                tokio::time::sleep(Duration::from_millis(DELAY_MS)).await;
            }
        }
    }

    Err(last_error.expect("uinput retry loop must store the last error"))
}

async fn handle_key_event(
    config: &AppConfig,
    event: KeyEvent,
    modifiers: &mut ModifierState,
    detector: &mut DoubleShiftDetector,
    buffer: &mut WordBuffer,
    input_sources: &InputSourceManager,
    atspi: Option<&AtspiBridge>,
    injector: &mut Injector,
) -> Result<bool> {
    let now = Instant::now();
    update_modifier_state(modifiers, event);

    if is_shift_key(event.code) {
        if event.value == 1 {
            detector.on_shift_press(now);
        } else if event.value == 0 && detector.on_shift_release(now) {
            return trigger_conversion(config, buffer, input_sources, atspi, injector).await;
        }
        return Ok(false);
    }

    if event.value != 1 {
        return Ok(false);
    }

    if key_clears_selection_cache(event.code, modifiers.shift) {
        if let Some(atspi) = atspi {
            atspi.clear_recent_text_selection().await;
        }
    }

    if modifiers.ctrl || modifiers.alt || modifiers.meta {
        detector.invalidate_sequence();
        buffer.push_break();
        return Ok(false);
    }

    detector.invalidate_sequence();

    match event.code {
        KeyCode::KEY_BACKSPACE => buffer.pop_last_char(),
        KeyCode::KEY_SPACE => buffer.push_literal(' '),
        KeyCode::KEY_TAB => buffer.push_literal('\t'),
        KeyCode::KEY_ENTER => buffer.push_literal('\n'),
        KeyCode::KEY_LEFT | KeyCode::KEY_RIGHT | KeyCode::KEY_UP | KeyCode::KEY_DOWN => {
            buffer.push_break()
        }
        KeyCode::KEY_DELETE | KeyCode::KEY_HOME | KeyCode::KEY_END | KeyCode::KEY_ESC => {
            buffer.push_break()
        }
        code => {
            if crate::keymap::key_to_char(code, Layout::Us, modifiers.shift).is_some() {
                buffer.push_char(code, modifiers.shift);
            }
        }
    }

    Ok(false)
}

async fn trigger_conversion(
    config: &AppConfig,
    buffer: &mut WordBuffer,
    input_sources: &InputSourceManager,
    atspi: Option<&AtspiBridge>,
    injector: &mut Injector,
) -> Result<bool> {
    let current_layout = input_sources.current_layout().await?;

    let selection_result = selection::try_handle_selection(atspi, current_layout).await;
    match selection_result {
        Ok(SelectionOutcome::Handled {
            target_layout,
            replacement_text,
        }) => {
            let target_name = input_sources.configured_name_for_layout(target_layout)?;
            ensure_layout_switched(input_sources, injector, &target_name).await?;
            tokio::time::sleep(Duration::from_millis(config.post_switch_delay_ms)).await;
            injector.type_text(target_layout, &replacement_text)?;
            return Ok(true);
        }
        Ok(SelectionOutcome::NoSelection | SelectionOutcome::Unsupported) => {}
        Err(error) => {
            warn!("selected-text conversion failed, falling back to last-word: {error:#}")
        }
    }

    let (target_layout, target_name) = input_sources.paired_target_layout(current_layout).await?;
    let direction = match (current_layout, target_layout) {
        (Layout::Us, Layout::Ru) => Direction::UsToRu,
        (Layout::Ru, Layout::Us) => Direction::RuToUs,
        _ => return Ok(false),
    };

    let Some(plan) = buffer.plan_conversion(current_layout, direction) else {
        return Ok(false);
    };
    ensure_layout_switched(input_sources, injector, &target_name).await?;
    tokio::time::sleep(Duration::from_millis(config.post_switch_delay_ms)).await;
    injector.backspace(plan.delete_count)?;
    injector.type_text(target_layout, &plan.replacement_text)?;
    Ok(true)
}

async fn ensure_layout_switched(
    input_sources: &InputSourceManager,
    injector: &mut Injector,
    target_name: &str,
) -> Result<()> {
    input_sources.switch_to_layout_name(target_name).await?;
    if input_sources
        .wait_for_layout_name(target_name, Duration::from_millis(350))
        .await?
    {
        return Ok(());
    }

    if input_sources.has_alt_shift_toggle().await? {
        warn!("gsettings layout switch did not take effect, trying Alt+Shift fallback");
        injector.alt_shift_toggle()?;
        if input_sources
            .wait_for_layout_name(target_name, Duration::from_millis(500))
            .await?
        {
            return Ok(());
        }
    }

    anyhow::bail!("failed to switch active layout to `{target_name}`");
}

#[derive(Debug, Default)]
struct ModifierState {
    shift: bool,
    ctrl: bool,
    alt: bool,
    meta: bool,
}

fn update_modifier_state(state: &mut ModifierState, event: KeyEvent) {
    let pressed = event.value != 0;
    match event.code {
        KeyCode::KEY_LEFTSHIFT | KeyCode::KEY_RIGHTSHIFT => state.shift = pressed,
        KeyCode::KEY_LEFTCTRL | KeyCode::KEY_RIGHTCTRL => state.ctrl = pressed,
        KeyCode::KEY_LEFTALT | KeyCode::KEY_RIGHTALT => state.alt = pressed,
        KeyCode::KEY_LEFTMETA | KeyCode::KEY_RIGHTMETA => state.meta = pressed,
        _ => {}
    }
}

fn is_shift_key(code: KeyCode) -> bool {
    matches!(code, KeyCode::KEY_LEFTSHIFT | KeyCode::KEY_RIGHTSHIFT)
}

fn key_clears_selection_cache(code: KeyCode, shifted: bool) -> bool {
    match code {
        KeyCode::KEY_BACKSPACE
        | KeyCode::KEY_SPACE
        | KeyCode::KEY_TAB
        | KeyCode::KEY_ENTER
        | KeyCode::KEY_DELETE
        | KeyCode::KEY_HOME
        | KeyCode::KEY_END
        | KeyCode::KEY_ESC => true,
        KeyCode::KEY_LEFT | KeyCode::KEY_RIGHT | KeyCode::KEY_UP | KeyCode::KEY_DOWN => !shifted,
        _ => crate::keymap::key_to_char(code, Layout::Us, shifted).is_some(),
    }
}

#[cfg(test)]
mod tests {
    use super::key_clears_selection_cache;
    use evdev::KeyCode;

    #[test]
    fn typed_char_clears_selection_cache() {
        assert!(key_clears_selection_cache(KeyCode::KEY_H, false));
    }

    #[test]
    fn shifted_navigation_keeps_selection_cache() {
        assert!(!key_clears_selection_cache(KeyCode::KEY_RIGHT, true));
    }

    #[test]
    fn plain_navigation_clears_selection_cache() {
        assert!(key_clears_selection_cache(KeyCode::KEY_RIGHT, false));
    }
}
