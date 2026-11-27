use openaction::{Action, Instance, OpenActionResult, async_trait};
use serde::{Deserialize, Serialize};

use crate::led_control;

#[derive(Serialize, Deserialize, Clone)]
#[serde(default)]
pub struct ColorSettings {
    pub color: String,
}
impl Default for ColorSettings {
    fn default() -> Self {
        Self { color: "#ffffff".into() }
    }
}

#[derive(Serialize, Deserialize, Clone)]
#[serde(default)]
pub struct BrightnessSettings {
    pub step: u8,
}
impl Default for BrightnessSettings {
    fn default() -> Self {
        Self { step: 5 }
    }
}

fn parse_hex(s: &str) -> Option<[u8; 3]> {
    let s = s.trim_start_matches('#');
    if s.len() != 6 {
        return None;
    }
    let v = u32::from_str_radix(s, 16).ok()?;
    Some([(v >> 16) as u8, (v >> 8) as u8, v as u8])
}

/// Key: set the knob LED color
pub struct SetLedColor;
#[async_trait]
impl Action for SetLedColor {
    const UUID: &'static str = "com.github.ambiso.opendeck-akp05.ledcolor";
    type Settings = ColorSettings;

    async fn key_down(&self, _i: &Instance, s: &Self::Settings) -> OpenActionResult<()> {
        if let Some(color) = parse_hex(&s.color) {
            led_control::set_color(color).await;
        }
        Ok(())
    }
}

/// Dial: turn to change the knob LED brightness
pub struct LedBrightness;
#[async_trait]
impl Action for LedBrightness {
    const UUID: &'static str = "com.github.ambiso.opendeck-akp05.ledbrightness";
    type Settings = BrightnessSettings;

    async fn will_appear(&self, i: &Instance, _s: &Self::Settings) -> OpenActionResult<()> {
        let b = led_control::brightness().await;
        i.set_title(Some(format!("{b}%")), None).await
    }

    async fn dial_rotate(
        &self,
        i: &Instance,
        s: &Self::Settings,
        ticks: i16,
        _pressed: bool,
    ) -> OpenActionResult<()> {
        let b = led_control::change_brightness(ticks * s.step.max(1) as i16).await;
        i.set_title(Some(format!("{b}%")), None).await
    }
}