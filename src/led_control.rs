use std::sync::LazyLock;
use tokio::sync::Mutex;

use crate::DEVICES;

struct LedState {
    brightness: u8,
    color: [u8; 3],
    saved: Option<u8>, // level to restore after a mute
}

use crate::led_config::LedMode;

static STATE: LazyLock<Mutex<LedState>> = LazyLock::new(|| {
    let cfg = crate::config::load();
    let color = match cfg.leds.mode {
        Some(LedMode::Static { colors }) => colors.first().copied().unwrap_or([255, 255, 255]),
        None => [255, 255, 255],
    };
    Mutex::new(LedState { brightness: cfg.leds.brightness, color, saved: None })
});

const DEVICE_MAX: u16 = 60; // brightness where the LEDs stop getting brighter

async fn apply(state: &LedState) {
    let level = ((state.brightness as u16 * DEVICE_MAX + 50) / 100) as u8;
    let devices = DEVICES.read().await;
    for (id, dev) in devices.iter() {
        let colors = vec![state.color; 4];
        let result = dev
            .with_awake_device(async |d| {
                d.set_led_brightness(level).await?;
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

pub async fn set_color(color: [u8; 3]) {
    let mut s = STATE.lock().await;
    s.color = color;
    apply(&s).await;
}

pub async fn toggle_mute() {
    let mut s = STATE.lock().await;
    match s.saved.take() {
        Some(prev) => s.brightness = prev,
        None if s.brightness == 0 => s.brightness = 50, // already 0: just turn it on
        None => {
            s.saved = Some(s.brightness);
            s.brightness = 0;
        }
    }
    apply(&s).await;
}

pub async fn change_brightness(delta: i16) -> u8 {
    let mut s = STATE.lock().await;
    if let Some(prev) = s.saved.take() {
        s.brightness = prev;
    }
    s.brightness = (s.brightness as i16 + delta).clamp(0, 100) as u8;
    apply(&s).await;
    s.brightness
}

pub async fn brightness() -> u8 {
    STATE.lock().await.brightness
}