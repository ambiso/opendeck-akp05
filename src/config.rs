use serde::Deserialize;
use std::fs;
use std::path::PathBuf;

use crate::led_config::LedConfig;

#[derive(Debug, Clone, Default, Deserialize)]
#[serde(default)]
pub struct Config {
    /// `None` — no vibration command sent; `Some(true|false)` — enable/disable
    /// on connect. Only applied on devices whose firmware supports it.
    pub vibration: Option<bool>,
    /// Override automatic BGPIC selection for connected devices.
    pub bgpic: Option<bool>,
    pub leds: LedConfig,
}

/// Loads `config.toml`, then applies `leds.toml` on top of it (if present) as
/// an override for just the `[leds]` section.
pub fn load() -> Config {
    let mut config = config_path("config.toml")
        .and_then(read_toml::<Config>)
        .unwrap_or_default();

    if let Some(leds) = config_path("leds.toml").and_then(read_toml::<LedConfig>) {
        config.leds = leds;
    }

    config.leds = config.leds.resolved_colors();
    config
}

fn read_toml<T: for<'de> Deserialize<'de>>(path: PathBuf) -> Option<T> {
    let contents = fs::read_to_string(&path).ok()?;
    match toml::from_str::<T>(&contents) {
        Ok(v) => Some(v),
        Err(e) => {
            log::warn!("Failed to parse {}: {e}", path.display());
            None
        }
    }
}

fn config_path(name: &str) -> Option<PathBuf> {
    Some(dirs::config_dir()?.join("opendeck-akp05").join(name))
}
