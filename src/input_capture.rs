use std::collections::HashSet;
use std::fs;
use std::path::{Path, PathBuf};
use std::thread;
use std::time::Duration;

use anyhow::{Context, Result};
use evdev::{Device, EventSummary, InputEvent, KeyCode};
use tokio::sync::mpsc::UnboundedSender;
use tracing::warn;

#[derive(Debug, Clone, Copy)]
pub struct KeyEvent {
    pub code: KeyCode,
    pub value: i32,
}

pub fn spawn(tx: UnboundedSender<KeyEvent>) {
    thread::spawn(move || {
        let mut known = HashSet::new();
        loop {
            if let Err(error) = scan_devices(&tx, &mut known) {
                warn!("input scan failed: {error:#}");
            }
            thread::sleep(Duration::from_secs(3));
        }
    });
}

fn scan_devices(tx: &UnboundedSender<KeyEvent>, known: &mut HashSet<PathBuf>) -> Result<()> {
    for entry in fs::read_dir("/dev/input").context("failed to read /dev/input")? {
        let path = entry?.path();
        if !is_event_node(&path) || known.contains(&path) {
            continue;
        }

        let device = match Device::open(&path) {
            Ok(device) => device,
            Err(error) => {
                warn!("failed to open {}: {error}", path.display());
                continue;
            }
        };

        if !looks_like_keyboard(&device) || device.name() == Some("lang-switcher virtual keyboard")
        {
            continue;
        }

        known.insert(path.clone());
        let tx = tx.clone();
        thread::spawn(move || read_device(path, device, tx));
    }
    Ok(())
}

fn read_device(path: PathBuf, mut device: Device, tx: UnboundedSender<KeyEvent>) {
    loop {
        match device.fetch_events() {
            Ok(events) => {
                for event in events {
                    if let Some(key_event) = to_key_event(event) {
                        if tx.send(key_event).is_err() {
                            return;
                        }
                    }
                }
            }
            Err(error) => {
                warn!("stopped reading {}: {error}", path.display());
                return;
            }
        }
        thread::sleep(Duration::from_millis(2));
    }
}

fn to_key_event(event: InputEvent) -> Option<KeyEvent> {
    let EventSummary::Key(_, code, value) = event.destructure() else {
        return None;
    };
    Some(KeyEvent { code, value })
}

fn is_event_node(path: &Path) -> bool {
    path.file_name()
        .and_then(|name| name.to_str())
        .is_some_and(|name| name.starts_with("event"))
}

fn looks_like_keyboard(device: &Device) -> bool {
    device
        .supported_keys()
        .is_some_and(|keys| keys.contains(KeyCode::KEY_A) && keys.contains(KeyCode::KEY_SPACE))
}
