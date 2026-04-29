use std::fs;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result, anyhow, bail};
use clap::{Parser, Subcommand};
use tracing_subscriber::EnvFilter;

use crate::config::AppConfig;
use crate::gnome::InputSourceManager;
use crate::selection;

#[derive(Debug, Parser)]
#[command(author, version, about)]
pub struct Cli {
    #[arg(long, global = true)]
    pub config_path: Option<PathBuf>,

    #[command(subcommand)]
    pub command: Commands,
}

#[derive(Debug, Subcommand)]
pub enum Commands {
    Run,
    Status,
    Doctor,
    Install {
        #[arg(long)]
        print_udev_rules: bool,
    },
    Config {
        #[command(subcommand)]
        command: ConfigCommands,
    },
}

#[derive(Debug, Subcommand)]
pub enum ConfigCommands {
    Path,
    Get { key: String },
    Set { key: String, value: String },
}

pub fn init_logging(level: &str) -> Result<()> {
    let filter = EnvFilter::try_from_default_env()
        .or_else(|_| EnvFilter::try_new(level))
        .context("failed to configure logging")?;

    tracing_subscriber::fmt()
        .with_env_filter(filter)
        .with_target(false)
        .try_init()
        .map_err(|error| anyhow!("failed to initialize logging: {error}"))?;

    Ok(())
}

pub async fn print_status(config: &AppConfig) -> Result<()> {
    let manager = InputSourceManager::new(config.layout_pair.clone());
    let state = manager.state().await?;
    let current = state.current_layout()?;

    println!("config: {}", config.path.display());
    println!("layouts: {:?}", state.layouts);
    println!("mru_layouts: {:?}", state.mru_layouts);
    println!("current_layout: {current}");
    println!(
        "double_shift_timeout_ms: {}",
        config.double_shift_timeout_ms
    );
    println!(
        "selection_mode: {}",
        selection::configured_mode(config.enable_selected_text)
    );
    println!("alt_shift_fallback: {}", config.enable_alt_shift_fallback);
    Ok(())
}

pub fn print_config_path(config: &AppConfig) -> Result<()> {
    println!("{}", config.path.display());
    Ok(())
}

pub fn config_get(config: &AppConfig, key: &str) -> Result<()> {
    match key {
        "double_shift_timeout_ms" => println!("{}", config.double_shift_timeout_ms),
        "max_shift_hold_ms" => println!("{}", config.max_shift_hold_ms),
        "buffer_len" => println!("{}", config.buffer_len),
        "post_switch_delay_ms" => println!("{}", config.post_switch_delay_ms),
        "enable_selected_text" => println!("{}", config.enable_selected_text),
        "enable_alt_shift_fallback" => println!("{}", config.enable_alt_shift_fallback),
        "log_level" => println!("{}", config.log_level),
        "layout_pair" => println!("{},{}", config.layout_pair[0], config.layout_pair[1]),
        _ => bail!("unknown config key: {key}"),
    }
    Ok(())
}

pub fn config_set(config: &mut AppConfig, key: &str, value: &str) -> Result<()> {
    match key {
        "double_shift_timeout_ms" => {
            config.double_shift_timeout_ms = value.parse().context("invalid integer")?
        }
        "max_shift_hold_ms" => {
            config.max_shift_hold_ms = value.parse().context("invalid integer")?
        }
        "buffer_len" => config.buffer_len = value.parse().context("invalid integer")?,
        "post_switch_delay_ms" => {
            config.post_switch_delay_ms = value.parse().context("invalid integer")?
        }
        "enable_selected_text" => {
            config.enable_selected_text = value.parse().context("invalid boolean")?
        }
        "enable_alt_shift_fallback" => {
            config.enable_alt_shift_fallback = value.parse().context("invalid boolean")?
        }
        "log_level" => config.log_level = value.to_owned(),
        "layout_pair" => {
            let pair: Vec<_> = value
                .split(',')
                .map(str::trim)
                .filter(|part| !part.is_empty())
                .collect();
            if pair.len() != 2 {
                bail!("layout_pair must contain two comma-separated layouts");
            }
            config.layout_pair = [pair[0].to_owned(), pair[1].to_owned()];
        }
        _ => bail!("unknown config key: {key}"),
    }

    config.save()?;
    println!("saved {}", config.path.display());
    Ok(())
}

pub fn install(config: &AppConfig, print_udev_rules: bool) -> Result<()> {
    let home = dirs::home_dir().context("failed to resolve home directory")?;
    let local_bin = home.join(".local/bin");
    let systemd_user = home.join(".config/systemd/user");
    let service_path = systemd_user.join("lang-switcher.service");
    let exe = std::env::current_exe().context("failed to discover current executable")?;

    fs::create_dir_all(&local_bin)?;
    fs::create_dir_all(&systemd_user)?;
    config.ensure_parent_dir()?;
    if !config.path.exists() {
        config.save()?;
    }

    let installed_bin = local_bin.join("lang-switcher");
    if exe != installed_bin {
        install_binary(&exe, &installed_bin)?;
    }

    let service = format!(
        "[Unit]\nDescription=lang-switcher daemon\nAfter=graphical-session.target\nPartOf=graphical-session.target\n\n[Service]\nType=simple\nExecStart={} --config-path {} run\nRestart=on-failure\nRestartSec=1\n\n[Install]\nWantedBy=default.target\n",
        installed_bin.display(),
        config.path.display()
    );
    fs::write(&service_path, service)
        .with_context(|| format!("failed to write {}", service_path.display()))?;

    println!("installed binary: {}", installed_bin.display());
    println!("installed service: {}", service_path.display());
    println!("next:");
    println!("  systemctl --user daemon-reload");
    println!("  systemctl --user enable --now lang-switcher.service");

    if print_udev_rules {
        println!();
        println!("{}", udev_rules());
    } else {
        println!("optional:");
        println!(
            "  {} install --print-udev-rules | sudo tee /etc/udev/rules.d/99-lang-switcher.rules",
            installed_bin.display()
        );
    }

    Ok(())
}

pub fn udev_rules() -> &'static str {
    "KERNEL==\"event*\", SUBSYSTEM==\"input\", GROUP=\"input\", MODE=\"0640\"\nKERNEL==\"uinput\", SUBSYSTEM==\"misc\", GROUP=\"input\", MODE=\"0660\""
}

fn install_binary(source: &Path, destination: &Path) -> Result<()> {
    let temp_name = format!(
        ".lang-switcher.tmp-{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|duration| duration.as_millis())
            .unwrap_or_default()
    );
    let temp_path = destination.with_file_name(temp_name);

    fs::copy(source, &temp_path).with_context(|| {
        format!(
            "failed to copy binary from {} to {}",
            source.display(),
            temp_path.display()
        )
    })?;

    fs::rename(&temp_path, destination).with_context(|| {
        format!(
            "failed to move binary from {} to {}",
            temp_path.display(),
            destination.display()
        )
    })?;

    Ok(())
}
