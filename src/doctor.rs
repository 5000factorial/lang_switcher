use std::fs::OpenOptions;
use std::io::ErrorKind;
use std::path::Path;

use anyhow::Result;
use evdev::Device;

use crate::config::AppConfig;
use crate::gnome::InputSourceManager;
use crate::selection;

pub async fn run(config: &AppConfig) -> Result<()> {
    println!("config: {}", config.path.display());
    println!(
        "session_type: {}",
        std::env::var("XDG_SESSION_TYPE").unwrap_or_else(|_| "<unknown>".to_owned())
    );
    println!(
        "desktop_session: {}",
        std::env::var("DESKTOP_SESSION").unwrap_or_else(|_| "<unknown>".to_owned())
    );
    println!(
        "wayland_display: {}",
        std::env::var("WAYLAND_DISPLAY").unwrap_or_else(|_| "<unset>".to_owned())
    );
    println!("input_access: {}", input_access());
    println!("uinput_access: {}", uinput_access());

    let sources = InputSourceManager::new(config.layout_pair.clone())
        .state()
        .await;
    match sources {
        Ok(state) => {
            println!("gnome_input_sources: {:?}", state.layouts);
            println!("gnome_mru_sources: {:?}", state.mru_layouts);
            println!("current_layout: {}", state.current_layout()?);
        }
        Err(error) => println!("gnome_input_sources: error: {error:#}"),
    }

    println!("pair: {:?}", config.layout_pair);
    println!(
        "selected_text_mode: {}",
        selection::configured_mode(config.enable_selected_text)
    );
    Ok(())
}

fn input_access() -> &'static str {
    let entries = match std::fs::read_dir("/dev/input") {
        Ok(entries) => entries,
        Err(error) if error.kind() == ErrorKind::NotFound => return "missing",
        Err(error) if error.kind() == ErrorKind::PermissionDenied => return "permission-denied",
        Err(_) => return "unavailable",
    };

    let mut saw_event_node = false;
    let mut saw_permission_denied = false;

    for entry in entries.flatten() {
        let path = entry.path();
        if !is_event_node(&path) {
            continue;
        }

        saw_event_node = true;
        match Device::open(&path) {
            Ok(_) => return "present",
            Err(error) if error.kind() == ErrorKind::PermissionDenied => {
                saw_permission_denied = true;
            }
            Err(_) => {}
        }
    }

    if saw_permission_denied {
        "permission-denied"
    } else if saw_event_node {
        "unreadable"
    } else {
        "missing"
    }
}

fn uinput_access() -> &'static str {
    match OpenOptions::new()
        .read(true)
        .write(true)
        .open("/dev/uinput")
    {
        Ok(_) => "present",
        Err(error) if error.kind() == ErrorKind::NotFound => "missing",
        Err(error) if error.kind() == ErrorKind::PermissionDenied => "permission-denied",
        Err(_) => "unavailable",
    }
}

fn is_event_node(path: &Path) -> bool {
    path.file_name()
        .and_then(|name| name.to_str())
        .is_some_and(|name| name.starts_with("event"))
}
