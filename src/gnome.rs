use anyhow::{Context, Result, anyhow, bail};
use tokio::process::Command;
use tokio::time::{Duration, Instant, sleep, timeout};

use crate::keymap::{Layout, parse_layout};

#[derive(Debug, Clone)]
pub struct InputSourceManager {
    layout_pair: [String; 2],
}

#[derive(Debug, Clone)]
pub struct InputSourceState {
    pub layouts: Vec<String>,
    pub mru_layouts: Vec<String>,
    pub current_index: usize,
}

impl InputSourceState {
    pub fn current_layout(&self) -> Result<String> {
        if let Some(current) = self.mru_layouts.first() {
            return Ok(current.clone());
        }

        self.layouts
            .get(self.current_index)
            .cloned()
            .ok_or_else(|| anyhow!("current input source index is out of bounds"))
    }
}

impl InputSourceManager {
    pub fn new(layout_pair: [String; 2]) -> Self {
        Self { layout_pair }
    }

    pub fn configured_name_for_layout(&self, layout: Layout) -> Result<String> {
        self.layout_pair
            .iter()
            .find(|name| parse_layout(name) == Some(layout))
            .cloned()
            .ok_or_else(|| anyhow!("layout `{layout:?}` is not part of the configured pair"))
    }

    pub async fn state(&self) -> Result<InputSourceState> {
        let current_raw = gsettings_get("current").await?;
        let sources_raw = gsettings_get("sources").await?;
        let mru_raw = gsettings_get("mru-sources").await?;
        let current_index = parse_current_index(&current_raw)?;
        let layouts = parse_sources(&sources_raw);
        let mru_layouts = parse_sources(&mru_raw);
        Ok(InputSourceState {
            layouts,
            mru_layouts,
            current_index,
        })
    }

    pub async fn current_layout(&self) -> Result<Layout> {
        let state = self.state().await?;
        let current = state.current_layout()?;
        parse_layout(&current).ok_or_else(|| anyhow!("unsupported current layout: {current}"))
    }

    pub async fn paired_target_layout(&self, current: Layout) -> Result<(Layout, String)> {
        let state = self.state().await?;
        let current_name = state.current_layout()?;

        let pair = [&self.layout_pair[0], &self.layout_pair[1]];
        if !pair.iter().any(|name| **name == current_name) {
            bail!(
                "current layout `{current_name}` is not part of the configured pair {:?}",
                self.layout_pair
            );
        }

        let target_name = if current_name == self.layout_pair[0] {
            self.layout_pair[1].clone()
        } else {
            self.layout_pair[0].clone()
        };

        let target_layout = parse_layout(&target_name)
            .ok_or_else(|| anyhow!("unsupported target layout: {target_name}"))?;
        let _ = current;
        Ok((target_layout, target_name))
    }

    pub async fn switch_to_layout_name(&self, target_name: &str) -> Result<()> {
        let state = self.state().await?;
        let Some(index) = state
            .layouts
            .iter()
            .position(|layout| layout == target_name)
        else {
            bail!("layout `{target_name}` not found in GNOME input sources");
        };

        run_gsettings_status(
            [
                "set",
                "org.gnome.desktop.input-sources",
                "current",
                &index.to_string(),
            ],
            GSETTINGS_SET_TIMEOUT,
            "set current layout",
        )
        .await?;
        Ok(())
    }

    pub async fn wait_for_layout_name(&self, target_name: &str, timeout: Duration) -> Result<bool> {
        let deadline = Instant::now() + timeout;
        loop {
            let state = self.state().await?;
            if state.current_layout()? == target_name {
                return Ok(true);
            }

            if Instant::now() >= deadline {
                return Ok(false);
            }

            sleep(Duration::from_millis(25)).await;
        }
    }

    pub async fn has_alt_shift_toggle(&self) -> Result<bool> {
        let raw = gsettings_get("xkb-options").await?;
        Ok(parse_string_list(&raw)
            .iter()
            .any(|option| option == "grp:alt_shift_toggle"))
    }
}

const GSETTINGS_GET_TIMEOUT: Duration = Duration::from_millis(1200);
const GSETTINGS_SET_TIMEOUT: Duration = Duration::from_millis(1500);

async fn gsettings_get(key: &str) -> Result<String> {
    let output = run_gsettings_output(
        ["get", "org.gnome.desktop.input-sources", key],
        GSETTINGS_GET_TIMEOUT,
        &format!("get for key `{key}`"),
    )
    .await?;

    Ok(output)
}

async fn run_gsettings_output<const N: usize>(
    args: [&str; N],
    deadline: Duration,
    description: &str,
) -> Result<String> {
    let mut command = Command::new("gsettings");
    command.kill_on_drop(true).args(args);

    let output = timeout(deadline, command.output())
        .await
        .map_err(|_| anyhow!("gsettings {description} timed out"))?
        .with_context(|| format!("failed to execute gsettings {description}"))?;

    if !output.status.success() {
        bail!("gsettings {description} failed");
    }

    Ok(String::from_utf8_lossy(&output.stdout).trim().to_owned())
}

async fn run_gsettings_status<const N: usize>(
    args: [&str; N],
    deadline: Duration,
    description: &str,
) -> Result<()> {
    let mut command = Command::new("gsettings");
    command.kill_on_drop(true).args(args);

    let status = timeout(deadline, command.status())
        .await
        .map_err(|_| anyhow!("gsettings {description} timed out"))?
        .with_context(|| format!("failed to execute gsettings {description}"))?;

    if !status.success() {
        bail!("gsettings {description} failed");
    }

    Ok(())
}

fn parse_current_index(raw: &str) -> Result<usize> {
    let value = raw.trim().strip_prefix("uint32 ").unwrap_or(raw.trim());
    value
        .parse::<usize>()
        .context("failed to parse gsettings current index")
}

fn parse_sources(raw: &str) -> Vec<String> {
    let mut quoted = Vec::new();
    let mut current = String::new();
    let mut in_quote = false;

    for ch in raw.chars() {
        match ch {
            '\'' if in_quote => {
                quoted.push(current.clone());
                current.clear();
                in_quote = false;
            }
            '\'' => in_quote = true,
            _ if in_quote => current.push(ch),
            _ => {}
        }
    }

    quoted
        .chunks(2)
        .filter_map(|pair| pair.get(1).cloned())
        .collect()
}

fn parse_string_list(raw: &str) -> Vec<String> {
    let mut values = Vec::new();
    let mut current = String::new();
    let mut in_quote = false;

    for ch in raw.chars() {
        match ch {
            '\'' if in_quote => {
                values.push(current.clone());
                current.clear();
                in_quote = false;
            }
            '\'' => in_quote = true,
            _ if in_quote => current.push(ch),
            _ => {}
        }
    }

    values
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_sources_output() {
        let parsed = parse_sources("[('xkb', 'ru'), ('xkb', 'us')]");
        assert_eq!(parsed, vec!["ru".to_owned(), "us".to_owned()]);
    }

    #[test]
    fn current_layout_prefers_mru_sources() {
        let state = InputSourceState {
            layouts: vec!["ru".to_owned(), "us".to_owned()],
            mru_layouts: vec!["us".to_owned(), "ru".to_owned()],
            current_index: 0,
        };
        assert_eq!(state.current_layout().unwrap(), "us");
    }

    #[test]
    fn parses_current_uint32() {
        assert_eq!(parse_current_index("uint32 1").unwrap(), 1);
    }

    #[test]
    fn parses_string_list_output() {
        let parsed = parse_string_list("['grp:alt_shift_toggle', 'compose:ralt']");
        assert_eq!(
            parsed,
            vec!["grp:alt_shift_toggle".to_owned(), "compose:ralt".to_owned()]
        );
    }
}
