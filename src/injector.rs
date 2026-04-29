use std::thread;
use std::time::Duration;

use anyhow::{Context, Result, anyhow};
use evdev::uinput::VirtualDevice;
use evdev::{AttributeSet, EventType, InputEvent, KeyCode};

use crate::keymap::{Layout, char_to_key};

#[derive(Debug)]
pub struct Injector {
    device: VirtualDevice,
}

impl Injector {
    pub fn new() -> Result<Self> {
        let mut keys = AttributeSet::<KeyCode>::new();
        for key in [
            KeyCode::KEY_LEFTALT,
            KeyCode::KEY_LEFTSHIFT,
            KeyCode::KEY_BACKSPACE,
            KeyCode::KEY_SPACE,
            KeyCode::KEY_ENTER,
            KeyCode::KEY_TAB,
            KeyCode::KEY_GRAVE,
            KeyCode::KEY_1,
            KeyCode::KEY_2,
            KeyCode::KEY_3,
            KeyCode::KEY_4,
            KeyCode::KEY_5,
            KeyCode::KEY_6,
            KeyCode::KEY_7,
            KeyCode::KEY_8,
            KeyCode::KEY_9,
            KeyCode::KEY_0,
            KeyCode::KEY_MINUS,
            KeyCode::KEY_EQUAL,
            KeyCode::KEY_Q,
            KeyCode::KEY_W,
            KeyCode::KEY_E,
            KeyCode::KEY_R,
            KeyCode::KEY_T,
            KeyCode::KEY_Y,
            KeyCode::KEY_U,
            KeyCode::KEY_I,
            KeyCode::KEY_O,
            KeyCode::KEY_P,
            KeyCode::KEY_LEFTBRACE,
            KeyCode::KEY_RIGHTBRACE,
            KeyCode::KEY_A,
            KeyCode::KEY_S,
            KeyCode::KEY_D,
            KeyCode::KEY_F,
            KeyCode::KEY_G,
            KeyCode::KEY_H,
            KeyCode::KEY_J,
            KeyCode::KEY_K,
            KeyCode::KEY_L,
            KeyCode::KEY_SEMICOLON,
            KeyCode::KEY_APOSTROPHE,
            KeyCode::KEY_BACKSLASH,
            KeyCode::KEY_Z,
            KeyCode::KEY_X,
            KeyCode::KEY_C,
            KeyCode::KEY_V,
            KeyCode::KEY_B,
            KeyCode::KEY_N,
            KeyCode::KEY_M,
            KeyCode::KEY_COMMA,
            KeyCode::KEY_DOT,
            KeyCode::KEY_SLASH,
        ] {
            keys.insert(key);
        }

        let device = VirtualDevice::builder()
            .context("failed to create uinput builder")?
            .name("lang-switcher virtual keyboard")
            .with_keys(&keys)
            .context("failed to configure uinput keyboard keys")?
            .build()
            .context("failed to create uinput virtual keyboard")?;

        Ok(Self { device })
    }

    pub fn alt_shift_toggle(&mut self) -> Result<()> {
        self.key_down(KeyCode::KEY_LEFTALT)?;
        self.key_down(KeyCode::KEY_LEFTSHIFT)?;
        self.key_up(KeyCode::KEY_LEFTSHIFT)?;
        self.key_up(KeyCode::KEY_LEFTALT)?;
        Ok(())
    }

    pub fn backspace(&mut self, count: usize) -> Result<()> {
        for _ in 0..count {
            self.tap_key(KeyCode::KEY_BACKSPACE)?;
        }
        Ok(())
    }

    pub fn type_text(&mut self, layout: Layout, text: &str) -> Result<()> {
        for ch in text.chars() {
            self.type_char(layout, ch)?;
        }
        Ok(())
    }

    fn type_char(&mut self, layout: Layout, ch: char) -> Result<()> {
        if ch == '\n' {
            return self.tap_key(KeyCode::KEY_ENTER);
        }
        if ch == '\t' {
            return self.tap_key(KeyCode::KEY_TAB);
        }
        let key = char_to_key(layout, ch)
            .ok_or_else(|| anyhow!("unsupported character for injection: {ch}"))?;
        if key.shifted {
            self.key_down(KeyCode::KEY_LEFTSHIFT)?;
        }
        self.tap_key(key.code)?;
        if key.shifted {
            self.key_up(KeyCode::KEY_LEFTSHIFT)?;
        }
        Ok(())
    }

    fn tap_key(&mut self, key: KeyCode) -> Result<()> {
        self.key_down(key)?;
        self.key_up(key)
    }

    fn key_down(&mut self, key: KeyCode) -> Result<()> {
        self.emit(key, 1)
    }

    fn key_up(&mut self, key: KeyCode) -> Result<()> {
        self.emit(key, 0)
    }

    fn emit(&mut self, key: KeyCode, value: i32) -> Result<()> {
        self.device
            .emit(&[InputEvent::new(EventType::KEY.0, key.code(), value)])
            .context("failed to emit input event")?;
        thread::sleep(Duration::from_millis(5));
        Ok(())
    }
}
