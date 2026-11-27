use std::sync::LazyLock;
use tokio::sync::Mutex;

use crate::DEVICES;

struct LedState {
    brightness: u8,
    color: [u8; 3],
}

static STATE: LazyLock<Mutex<LedState>> = LazyLock::new(|| {
    Mutex::new(LedState { brightness: 100, color: [255, 255, 255] })
});

async fn apply(state: &LedState) {
    let devices = DEVICES.read().await;
    for (id, dev) in devices.iter() {
        let colors = vec![state.color; 4];
        let result = dev
            .with_awake_device(async |d| {
                d.set_led_brightness(state.brightness).await?;
                d.set_led_colors(&colors).await
            })
            .await;
        if let Err(err) = result {
            dev.handle_error(id, err);
        }
    }
}

pub async fn set_color(color: [u8; 3]) {
    let mut s = STATE.lock().await;
    s.color = color;
    apply(&s).await;
}

pub async fn change_brightness(delta: i16) -> u8 {
    let mut s = STATE.lock().await;
    s.brightness = (s.brightness as i16 + delta).clamp(0, 100) as u8;
    apply(&s).await;
    s.brightness
}

pub async fn brightness() -> u8 {
    STATE.lock().await.brightness
}