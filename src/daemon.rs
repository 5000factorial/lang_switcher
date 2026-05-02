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
    let injector = create_injector_with_retry().await?;
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

    let mut runtime = RuntimeState {
        buffer: WordBuffer::new(config.buffer_len),
        detector: DoubleShiftDetector::new(
            config.double_shift_timeout_ms,
            config.max_shift_hold_ms,
        ),
        modifiers: ModifierState::default(),
        atspi,
        injector,
    };

    loop {
        tokio::select! {
            _ = tokio::signal::ctrl_c() => {
                info!("received shutdown signal");
                return Ok(());
            }
            Some(event) = rx.recv() => {
                match handle_key_event(&config, event, &input_sources, &mut runtime).await {
                    Ok(consumed) => {
                        if consumed {
                            runtime.buffer.clear();
                        }
                    }
                    Err(error) => {
                        warn!("input event handling failed: {error:#}");
                        runtime
                            .recover_after_event_error(should_recreate_injector(&error))
                            .await;
                    }
                }
            }
        }
    }
}

async fn create_injector_with_retry() -> Result<Injector> {
    create_injector_with_options(10, 500).await
}

async fn create_injector_with_options(attempts: usize, delay_ms: u64) -> Result<Injector> {
    let mut last_error = None;
    for attempt in 1..=attempts {
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
                    "failed to initialize uinput on attempt {attempt}/{attempts}; retrying in {} ms",
                    delay_ms
                );
                tokio::time::sleep(Duration::from_millis(delay_ms)).await;
            }
        }
    }

    Err(last_error.expect("uinput retry loop must store the last error"))
}

async fn handle_key_event(
    config: &AppConfig,
    event: KeyEvent,
    input_sources: &InputSourceManager,
    runtime: &mut RuntimeState,
) -> Result<bool> {
    let now = Instant::now();
    update_modifier_state(&mut runtime.modifiers, event);

    if is_shift_key(event.code) {
        if event.value == 1 {
            runtime.detector.on_shift_press(now);
        } else if event.value == 0 && runtime.detector.on_shift_release(now) {
            return trigger_conversion(config, input_sources, runtime).await;
        }
        return Ok(false);
    }

    if event.value != 1 {
        return Ok(false);
    }

    runtime
        .clear_selection_hint_on_plain_input(event.code)
        .await;

    if runtime.modifiers.ctrl || runtime.modifiers.alt || runtime.modifiers.meta {
        runtime.clear_selection_hint().await;
        runtime.detector.invalidate_sequence();
        runtime.buffer.push_break();
        return Ok(false);
    }

    runtime.detector.invalidate_sequence();
    runtime.record_input(event.code);

    Ok(false)
}

async fn trigger_conversion(
    config: &AppConfig,
    input_sources: &InputSourceManager,
    runtime: &mut RuntimeState,
) -> Result<bool> {
    let current_layout = input_sources.current_layout().await?;

    if try_selected_text_conversion(config, input_sources, runtime, current_layout).await? {
        return Ok(true);
    }

    try_last_word_conversion(config, input_sources, runtime, current_layout).await
}

async fn try_selected_text_conversion(
    config: &AppConfig,
    input_sources: &InputSourceManager,
    runtime: &mut RuntimeState,
    current_layout: Layout,
) -> Result<bool> {
    let selection_result =
        selection::try_handle_selection(runtime.atspi.as_ref(), current_layout).await;
    match selection_result {
        Ok(SelectionOutcome::Handled {
            target_layout,
            replacement_text,
        }) => {
            runtime
                .switch_layout(input_sources, config, target_layout)
                .await?;
            runtime
                .injector
                .type_text(target_layout, &replacement_text)?;
            Ok(true)
        }
        Ok(SelectionOutcome::NoSelection | SelectionOutcome::Unsupported) => Ok(false),
        Err(error) => {
            warn!("selected-text conversion failed, falling back to last-word: {error:#}");
            Ok(false)
        }
    }
}

async fn try_last_word_conversion(
    config: &AppConfig,
    input_sources: &InputSourceManager,
    runtime: &mut RuntimeState,
    current_layout: Layout,
) -> Result<bool> {
    let (target_layout, target_name) = input_sources.paired_target_layout(current_layout).await?;
    let Some(direction) = conversion_direction(current_layout, target_layout) else {
        return Ok(false);
    };

    let Some(plan) = runtime.buffer.plan_conversion(current_layout, direction) else {
        return Ok(false);
    };
    runtime
        .switch_layout_name(input_sources, config, &target_name)
        .await?;
    runtime.injector.backspace(plan.delete_count)?;
    runtime
        .injector
        .type_text(target_layout, &plan.replacement_text)?;
    Ok(true)
}

async fn ensure_layout_switched(
    input_sources: &InputSourceManager,
    injector: &mut Injector,
    target_name: &str,
    enable_alt_shift_fallback: bool,
) -> Result<()> {
    input_sources.switch_to_layout_name(target_name).await?;
    if input_sources
        .wait_for_layout_name(target_name, Duration::from_millis(350))
        .await?
    {
        return Ok(());
    }

    if enable_alt_shift_fallback && input_sources.has_alt_shift_toggle().await? {
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

struct RuntimeState {
    buffer: WordBuffer,
    detector: DoubleShiftDetector,
    modifiers: ModifierState,
    atspi: Option<AtspiBridge>,
    injector: Injector,
}

impl RuntimeState {
    async fn clear_selection_hint(&self) {
        if let Some(atspi) = self.atspi.as_ref() {
            atspi.clear_recent_text_selection().await;
        }
    }

    async fn clear_selection_hint_on_plain_input(&self, code: KeyCode) {
        if key_clears_selection_cache(code, self.modifiers.shift) {
            self.clear_selection_hint().await;
        }
    }

    async fn recover_after_event_error(&mut self, recreate_injector: bool) {
        self.buffer.clear();
        self.detector.reset();
        self.clear_selection_hint().await;

        if !recreate_injector {
            return;
        }

        match create_injector_with_options(3, 250).await {
            Ok(injector) => {
                self.injector = injector;
                info!("reinitialized uinput after event error");
            }
            Err(error) => warn!("failed to reinitialize uinput after event error: {error:#}"),
        }
    }

    fn record_input(&mut self, code: KeyCode) {
        match code {
            KeyCode::KEY_BACKSPACE => self.buffer.pop_last_char(),
            KeyCode::KEY_SPACE => self.buffer.push_literal(' '),
            KeyCode::KEY_TAB => self.buffer.push_literal('\t'),
            KeyCode::KEY_ENTER => self.buffer.push_literal('\n'),
            KeyCode::KEY_LEFT | KeyCode::KEY_RIGHT | KeyCode::KEY_UP | KeyCode::KEY_DOWN => {
                self.buffer.push_break()
            }
            KeyCode::KEY_DELETE | KeyCode::KEY_HOME | KeyCode::KEY_END | KeyCode::KEY_ESC => {
                self.buffer.push_break()
            }
            code => {
                if crate::keymap::key_to_char(code, Layout::Us, self.modifiers.shift).is_some() {
                    self.buffer.push_char(code, self.modifiers.shift);
                }
            }
        }
    }

    async fn switch_layout(
        &mut self,
        input_sources: &InputSourceManager,
        config: &AppConfig,
        target_layout: Layout,
    ) -> Result<()> {
        let target_name = input_sources.configured_name_for_layout(target_layout)?;
        self.switch_layout_name(input_sources, config, &target_name)
            .await
    }

    async fn switch_layout_name(
        &mut self,
        input_sources: &InputSourceManager,
        config: &AppConfig,
        target_name: &str,
    ) -> Result<()> {
        ensure_layout_switched(
            input_sources,
            &mut self.injector,
            target_name,
            config.enable_alt_shift_fallback,
        )
        .await?;
        tokio::time::sleep(Duration::from_millis(config.post_switch_delay_ms)).await;
        Ok(())
    }
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

fn conversion_direction(source: Layout, target: Layout) -> Option<Direction> {
    match (source, target) {
        (Layout::Us, Layout::Ru) => Some(Direction::UsToRu),
        (Layout::Ru, Layout::Us) => Some(Direction::RuToUs),
        _ => None,
    }
}

fn should_recreate_injector(error: &anyhow::Error) -> bool {
    error.chain().any(|cause| {
        let text = cause.to_string();
        text.contains("uinput") || text.contains("failed to emit input event")
    })
}

#[cfg(test)]
mod tests {
    use super::{conversion_direction, key_clears_selection_cache, should_recreate_injector};
    use crate::keymap::{Direction, Layout};
    use anyhow::anyhow;
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

    #[test]
    fn detects_supported_conversion_direction() {
        assert_eq!(
            conversion_direction(Layout::Us, Layout::Ru),
            Some(Direction::UsToRu)
        );
        assert_eq!(
            conversion_direction(Layout::Ru, Layout::Us),
            Some(Direction::RuToUs)
        );
        assert_eq!(conversion_direction(Layout::Us, Layout::Us), None);
    }

    #[test]
    fn detects_when_injector_recovery_is_needed() {
        assert!(should_recreate_injector(&anyhow!(
            "failed to create uinput virtual keyboard"
        )));
        assert!(should_recreate_injector(&anyhow!(
            "failed to emit input event"
        )));
        assert!(!should_recreate_injector(&anyhow!(
            "failed to switch active layout to `ru`"
        )));
    }
}
