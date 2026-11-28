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
        led_control::brightness().await;
        show_gauge(i).await
    }

    async fn dial_rotate(
        &self,
        i: &Instance,
        s: &Self::Settings,
        ticks: i16,
        _pressed: bool,
    ) -> OpenActionResult<()> {
        led_control::change_brightness(ticks * s.step.max(1) as i16).await;
        show_gauge(i).await
    }

    async fn dial_down(&self, i: &Instance, _s: &Self::Settings) -> OpenActionResult<()> {
        led_control::toggle_mute().await;
        show_gauge(i).await
    }
}

fn gauge_svg(pct: u8, color: [u8; 3]) -> String {
    let fill = pct as u32 * 160 / 100;
    format!(
        "<svg xmlns='http://www.w3.org/2000/svg' width='200' height='100' viewBox='0 0 200 100'>\
         <rect width='200' height='100' fill='#111'/>\
         <text x='100' y='52' font-family='sans-serif' font-size='38' font-weight='bold' \
         fill='white' text-anchor='middle'>{pct}%</text>\
         <rect x='20' y='68' width='160' height='12' rx='6' fill='#333'/>\
         <rect x='20' y='68' width='{fill}' height='12' rx='6' fill='rgb({},{},{})'/>\
         </svg>",
        color[0], color[1], color[2]
    )
}

fn svg_data_url(svg: &str) -> String {
    let mut out = String::from("data:image/svg+xml;charset=utf8,");
    for b in svg.bytes() {
        if b.is_ascii_alphanumeric() || b"-_.~".contains(&b) {
            out.push(b as char);
        } else {
            out.push_str(&format!("%{b:02X}"));
        }
    }
    out
}

async fn show_gauge(i: &Instance) -> OpenActionResult<()> {
    let (pct, color) = led_control::state().await;
    i.set_image(Some(svg_data_url(&gauge_svg(pct, color))), None).await
}