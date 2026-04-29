use anyhow::Result;

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
    println!("input_access: {}", access("/dev/input"));
    println!("uinput_access: {}", access("/dev/uinput"));

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

fn access(path: &str) -> &'static str {
    match std::fs::metadata(path) {
        Ok(_) => "present",
        Err(_) => "missing",
    }
}
