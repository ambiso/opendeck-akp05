use std::{fs, path::PathBuf, sync::LazyLock};

use mirajazz::{device::Device, error::MirajazzError};
use serde::{Deserialize, Serialize};
use tokio::sync::Mutex;

use crate::{DEVICES, led_config::LedMode};

/// Device brightness at which the LEDs stop getting brighter
const DEVICE_MAX: u16 = 60;

#[derive(Serialize, Deserialize)]
struct Saved {
    brightness: u8,
    color: [u8; 3],
}

struct LedState {
    brightness: u8,
    color: [u8; 3],
    saved: Option<u8>, // level to restore after a mute
    active: bool,      // false = never configured, so leave the device's own effect alone
}

fn state_path() -> Option<PathBuf> {
    dirs::config_dir().map(|d| d.join("opendeck-akp05").join("led_state.toml"))
}

fn load_saved() -> Option<Saved> {
    let text = fs::read_to_string(state_path()?).ok()?;
    toml::from_str(&text).ok()
}

fn persist(s: &LedState) {
    let Some(path) = state_path() else { return };
    // While muted, store the real level so it comes back after a restart
    let saved = Saved { brightness: s.saved.unwrap_or(s.brightness), color: s.color };
    if let Some(dir) = path.parent() {
        let _ = fs::create_dir_all(dir);
    }
    if let Ok(text) = toml::to_string(&saved) {
        let _ = fs::write(path, text);
    }
}

static STATE: LazyLock<Mutex<LedState>> = LazyLock::new(|| {
    if let Some(s) = load_saved() {
        return Mutex::new(LedState {
            brightness: s.brightness.min(100),
            color: s.color,
            saved: None,
            active: true,
        });
    }
    let cfg = crate::config::load();
    let (color, active) = match cfg.leds.mode {
        Some(LedMode::Static { colors }) => (colors.first().copied().unwrap_or([255u8; 3]), true),
        None => ([255u8; 3], false),
    };
    Mutex::new(LedState { brightness: cfg.leds.brightness, color, saved: None, active })
});

fn level(brightness: u8) -> u8 {
    ((brightness as u16 * DEVICE_MAX + 50) / 100) as u8
}

/// Called when the deck connects, so the saved look is applied at startup
pub async fn apply_to_device(device: &Device) -> Result<(), MirajazzError> {
    let s = STATE.lock().await;
    if !s.active {
        return Ok(());
    }
    device.set_led_brightness(level(s.brightness)).await?;
    device.set_led_colors(&vec![s.color; 4]).await
}

async fn apply(s: &LedState) {
    let lvl = level(s.brightness);
    let devices = DEVICES.read().await;
    for (id, dev) in devices.iter() {
        let colors = vec![s.color; 4];
        let result = dev
            .with_awake_device(async |d| {
                d.set_led_brightness(lvl).await?;
                d.set_led_colors(&colors).await
            })
            .await;
        if let Err(err) = result {
            dev.handle_error(id, err);
        }
    }
}

pub async fn state() -> (u8, [u8; 3]) {
    let s = STATE.lock().await;
    (s.brightness, s.color)
}

pub async fn brightness() -> u8 {
    STATE.lock().await.brightness
}

pub async fn set_color(color: [u8; 3]) {
    let mut s = STATE.lock().await;
    s.color = color;
    s.active = true;
    apply(&s).await;
    persist(&s);
}

pub async fn change_brightness(delta: i16) -> u8 {
    let mut s = STATE.lock().await;
    if let Some(prev) = s.saved.take() {
        s.brightness = prev;
    }
    s.brightness = (s.brightness as i16 + delta).clamp(0, 100) as u8;
    s.active = true;
    apply(&s).await;
    persist(&s);
    s.brightness
}

pub async fn toggle_mute() {
    let mut s = STATE.lock().await;
    match s.saved.take() {
        Some(prev) => s.brightness = prev,
        None if s.brightness == 0 => s.brightness = 50,
        None => {
            s.saved = Some(s.brightness);
            s.brightness = 0;
        }
    }
    s.active = true;
    apply(&s).await;
    persist(&s);
}