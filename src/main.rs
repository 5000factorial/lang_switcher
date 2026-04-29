use anyhow::Result;
use clap::Parser;
use lang_switcher::cli::{Cli, Commands, ConfigCommands};

#[tokio::main]
async fn main() -> Result<()> {
    let cli = Cli::parse();
    let config_path = cli.config_path.clone();
    let mut config = lang_switcher::config::AppConfig::load_or_default(config_path.as_deref())?;

    lang_switcher::cli::init_logging(config.log_level.as_str())?;

    match cli.command {
        Commands::Run => lang_switcher::daemon::run(config).await?,
        Commands::Status => lang_switcher::cli::print_status(&config).await?,
        Commands::Doctor => lang_switcher::doctor::run(&config).await?,
        Commands::Install { print_udev_rules } => {
            lang_switcher::cli::install(&config, print_udev_rules)?
        }
        Commands::Config { command } => match command {
            ConfigCommands::Path => lang_switcher::cli::print_config_path(&config)?,
            ConfigCommands::Get { key } => lang_switcher::cli::config_get(&config, &key)?,
            ConfigCommands::Set { key, value } => {
                lang_switcher::cli::config_set(&mut config, &key, &value)?
            }
        },
    }

    Ok(())
}
