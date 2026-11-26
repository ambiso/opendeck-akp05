use std::{
    sync::{Arc, Mutex},
    time::Duration,
};

use data_url::DataUrl;
use image::{
    DynamicImage, GenericImageView, RgbImage, imageops::FilterType, load_from_memory_with_format,
};
use mirajazz::{
    device::Device,
    error::MirajazzError,
    images::convert_image_with_format,
    state::DeviceStateUpdate,
    types::{ImageFormat, ImageMirroring, ImageMode, ImageRotation},
};
use openaction::{device_plugin, global_events::SetImageEvent};
use tokio::time::{interval, timeout};
use tokio_util::sync::CancellationToken;

use crate::{
    DEVICES, config, led_config,
    managed_device::ManagedDevice,
    mappings::{
        COL_COUNT, CandidateDevice, DEVICE_TYPE, ENCODER_COUNT, FullStrip, KEY_COUNT, Kind,
        ROW_COUNT,
    },
};

fn full_strip_format(size: (usize, usize)) -> ImageFormat {
    ImageFormat {
        mode: ImageMode::JPEG,
        size,
        rotation: ImageRotation::Rot180,
        mirror: ImageMirroring::None,
    }
}

/// Initializes a device and listens for events
pub async fn device_task(candidate: CandidateDevice, token: CancellationToken) {
    log::info!("Running device task for {:?}", candidate);

    let initialize = async {
        let device = connect(&candidate).await?;
        let cfg = config::load();
        let firmware = device.firmware_version.as_deref();
        let full_strip = candidate.kind.full_strip(firmware, cfg.bgpic);

        if let Some(full_strip) = full_strip {
            log::info!(
                "Using full-width touch strip via {full_strip:?} on {:04x}:{:04x}, firmware {}",
                device.vid,
                device.pid,
                firmware
                    .map(|version| version.trim_end_matches('\0'))
                    .unwrap_or("unknown"),
            );
        }

        let strip = full_strip.map(|mode| StripCanvas::new(mode, cfg.auto_crop));
        device.set_brightness(50).await?;
        clear_display(&device, strip.as_ref()).await?;

        log::info!("Applying config: {:?}", cfg);
        if let Some(led_config::LedMode::Static { colors }) = cfg.leds.mode {
            device.set_led_brightness(cfg.leds.brightness).await?;
            device.set_led_colors(&colors).await?;
        }

        if let Some(enabled) = cfg.vibration {
            if candidate.kind.supports_vibration() {
                log::info!("Setting vibration: {}", enabled);
                device.set_vibration(enabled).await?;
            } else {
                log::warn!(
                    "Ignoring vibration config: {} does not support it",
                    candidate.kind.human_name()
                );
            }
        }

        Ok((device, strip))
    };
    let device: Result<_, MirajazzError> = tokio::select! {
        biased;
        _ = token.cancelled() => return,
        result = initialize => result,
    };
    let (device, strip) = match device {
        Ok(device) => device,
        Err(err) => {
            log::error!("Device {} initialization failed: {err}", candidate.id);
            return;
        }
    };

    let device = Arc::new(ManagedDevice::new(device, token.clone(), strip));
    tokio::select! {
        biased;
        _ = token.cancelled() => {},
        _ = async {
            // Publish before registering: OpenDeck may immediately send images.
            DEVICES.write().await.insert(candidate.id.clone(), device.clone());
            if let Err(err) = device_plugin::register_device(
                candidate.id.clone(), candidate.kind.human_name(),
                ROW_COUNT as u8, COL_COUNT as u8, ENCODER_COUNT as u8, DEVICE_TYPE,
            ).await {
                log::error!("Device {} registration failed: {err}", candidate.id);
                return;
            }
            tokio::select! {
                _ = device_events_task(&candidate, &device) => {},
                _ = keepalive_task(&candidate, &device) => {},
            }
        } => {},
    };

    token.cancel();
    log::info!("Shutting down device {:?}", candidate);

    if timeout(Duration::from_secs(2), device.shutdown())
        .await
        .is_err()
    {
        log::warn!("Device {} shutdown timed out", candidate.id);
    }

    log::info!("Device task finished for {:?}", candidate);
}

pub async fn connect(candidate: &CandidateDevice) -> Result<Device, MirajazzError> {
    let result = Device::connect(
        &candidate.dev,
        candidate.kind.protocol_version(),
        KEY_COUNT,
        ENCODER_COUNT,
    )
    .await;

    match result {
        Ok(device) => {
            Ok(device
                .with_supports_both_encoder_states(candidate.kind.supports_both_encoder_states()))
        }
        Err(e) => {
            log::error!("Error while connecting to device: {e}");

            Err(e)
        }
    }
}

/// Handles events from device to OpenDeck
async fn device_events_task(
    candidate: &CandidateDevice,
    device: &ManagedDevice,
) -> Result<(), MirajazzError> {
    log::info!("Connecting to {} for incoming events", candidate.id);

    let reader = device.get_reader();

    log::info!("Connected to {} for incoming events", candidate.id);

    log::info!("Reader is ready for {}", candidate.id);

    loop {
        log::debug!("Reading updates...");

        let updates = match reader.read(None).await {
            Ok(updates) => updates,
            Err(e) => {
                if !device.handle_error(&candidate.id, e) {
                    break;
                }

                continue;
            }
        };

        for update in updates {
            log::debug!("New update: {:#?}", update);

            let id = candidate.id.clone();

            match update {
                DeviceStateUpdate::ButtonDown(key) => {
                    device_plugin::key_down(id, key).await.unwrap()
                }
                DeviceStateUpdate::ButtonUp(key) => device_plugin::key_up(id, key).await.unwrap(),
                DeviceStateUpdate::EncoderDown(encoder) => {
                    device_plugin::encoder_down(id, encoder).await.unwrap();
                }
                DeviceStateUpdate::EncoderUp(encoder) => {
                    device_plugin::encoder_up(id, encoder).await.unwrap();
                }
                DeviceStateUpdate::EncoderTwist(encoder, val) => {
                    device_plugin::encoder_change(id, encoder, val as i16)
                        .await
                        .unwrap();
                }
            }
        }
    }

    Ok(())
}

/// Sends periodic keepalives to the device to maintain connection
async fn keepalive_task(
    candidate: &CandidateDevice,
    device: &ManagedDevice,
) -> Result<(), MirajazzError> {
    let mut interval = interval(Duration::from_secs(10));

    loop {
        interval.tick().await;

        log::debug!("Sending keepalive to {}", candidate.id);

        let Err(e) = device
            .with_awake_device(async |device| device.keep_alive().await)
            .await
        else {
            continue;
        };
        if !device.handle_error(&candidate.id, e) {
            break;
        }
    }

    Ok(())
}

fn map_position(mut position: u8, is_encoder: bool) -> Result<u8, MirajazzError> {
    if is_encoder {
        position += 10;
    }
    let position = match position {
        0 => 10,
        1 => 11,
        2 => 12,
        3 => 13,
        4 => 14,
        5 => 5,
        6 => 6,
        7 => 7,
        8 => 8,
        9 => 9,
        10 => 0,
        11 => 1,
        12 => 2,
        13 => 3,
        _ => {
            log::error!("Invalid key position");
            return Err(MirajazzError::BadData);
        }
    };
    Ok(position)
}

/// Handles different combinations of "set image" event, including clearing the specific buttons and whole device
pub async fn handle_set_image(
    device: &Device,
    evt: SetImageEvent,
    strip: Option<&StripCanvas>,
) -> Result<(), MirajazzError> {
    let is_encoder = evt.controller.as_deref() == Some("Encoder");
    let kind = Kind::from_vid_pid(device.vid, device.pid).unwrap();
    let encoder_strip = strip.filter(|_| is_encoder);
    match (evt.position, evt.image) {
        (Some(position), Some(image)) => {
            log::debug!("Setting image for button {}", position);
            let dial = position;
            let position = map_position(position, is_encoder)?;

            // OpenDeck sends image as a data url, so parse it using a library
            let url = DataUrl::process(image.as_str()).unwrap(); // Isn't expected to fail, so unwrap it is
            let (body, _fragment) = url.decode_to_vec().unwrap(); // Same here

            // Allow only image/jpeg mime for now
            if url.mime_type().subtype != "jpeg" {
                log::error!("Incorrect mime type: {}", url.mime_type());

                return Ok(()); // Not a fatal error, enough to just log it
            }

            let image = load_from_memory_with_format(body.as_slice(), image::ImageFormat::Jpeg)?;

            if let Some(strip) = encoder_strip {
                return strip.set_dial(device, dial, Some(image)).await;
            }

            let format = if is_encoder {
                kind.touch_image_format()
            } else {
                kind.image_format()
            };
            // The panel conversion stretches into `format.size`, which distorts
            // dial images (OpenDeck renders 200x100, the N4 strip is 176x112):
            // pre-fit encoder images preserving aspect ratio, so that stretch
            // becomes a no-op.
            let image = if is_encoder {
                fit_into(image, format.size, FilterType::Nearest)
            } else {
                image
            };

            device.set_button_image(position, format, image).await?;
            device.flush().await?;
        }
        (Some(position), None) => {
            if let Some(strip) = encoder_strip {
                return strip.set_dial(device, position, None).await;
            }
            let position = map_position(position, is_encoder)?;
            device.clear_button_image(position).await?;
            device.flush().await?;
        }
        (None, None) => {
            clear_display(device, strip).await?;
        }
        _ => {}
    }

    Ok(())
}

/// One continuous strip canvas per connection, with a quarter for each dial.
pub struct StripCanvas {
    mode: FullStrip,
    auto_crop: bool,
    pixels: Mutex<RgbImage>,
}

impl StripCanvas {
    fn new(mode: FullStrip, auto_crop: bool) -> Self {
        let (width, height) = mode.size();
        Self {
            mode,
            auto_crop,
            pixels: Mutex::new(RgbImage::new(width as u32, height as u32)),
        }
    }

    fn update(&self, dial: u8, image: Option<DynamicImage>) -> Result<DynamicImage, MirajazzError> {
        let dial = usize::from(dial);
        if dial >= ENCODER_COUNT {
            log::error!("Invalid dial position {dial}");
            return Err(MirajazzError::BadData);
        }

        let (strip_width, height) = self.mode.size();
        let width = strip_width / ENCODER_COUNT;
        let tile = match image {
            Some(image) => {
                let image = if self.auto_crop {
                    crop_padding(image)
                } else {
                    image
                };
                fit_into(image, (width, height), FilterType::CatmullRom).into_rgb8()
            }
            None => RgbImage::new(width as u32, height as u32),
        };
        let mut pixels = self.pixels.lock().unwrap();
        image::imageops::replace(&mut *pixels, &tile, (dial * width) as i64, 0);
        Ok(DynamicImage::ImageRgb8(pixels.clone()))
    }

    async fn set_dial(
        &self,
        device: &Device,
        dial: u8,
        image: Option<DynamicImage>,
    ) -> Result<(), MirajazzError> {
        let image = self.update(dial, image)?;
        match self.mode {
            FullStrip::ImageSlot(index) => {
                device
                    .set_button_image(index, full_strip_format(self.mode.size()), image)
                    .await?;
                device.flush().await
            }
            FullStrip::Bgpic | FullStrip::PanelBgpic => send_strip_bgpic(device, image).await,
        }
    }
}

async fn clear_display(device: &Device, strip: Option<&StripCanvas>) -> Result<(), MirajazzError> {
    let background = strip.and_then(|strip| {
        let mut pixels = strip.pixels.lock().unwrap();
        pixels.fill(0);
        strip
            .mode
            .uses_bgpic()
            .then(|| DynamicImage::ImageRgb8(pixels.clone()))
    });
    device.clear_all_button_images().await?;
    if let Some(background) = background {
        return send_strip_bgpic(device, background).await;
    }
    device.flush().await
}

async fn send_strip_bgpic(device: &Device, image: DynamicImage) -> Result<(), MirajazzError> {
    let (width, height) = (image.width() as usize, image.height() as usize);
    let jpeg = convert_image_with_format(full_strip_format((width, height)), image).await?;
    let len = u32::try_from(jpeg.len()).map_err(|_| MirajazzError::BadData)?;
    let mut report = b"\0CRT\0\0BGPIC".to_vec();
    report.extend_from_slice(&len.to_be_bytes());
    // BGPIC encodes x, y, width and height as big-endian u16s, then 0 and the layer.
    for value in [0, 0, width as u16, height as u16] {
        report.extend_from_slice(&value.to_be_bytes());
    }
    report.extend_from_slice(&[0, 0]);
    device.write_extended_data(&mut report).await?;

    // All supported devices use 1024-byte HID payloads after the report ID.
    for chunk in jpeg.chunks(1024) {
        report[1..1 + chunk.len()].copy_from_slice(chunk);
        report[1 + chunk.len()..].fill(0);
        device.write_data(&report).await?;
    }

    device
        .write_extended_data(&mut b"\0CRT\0\0STP".to_vec())
        .await
}

/// Remove matching dark margins around the artwork while preserving its aspect ratio.
fn crop_padding(image: DynamicImage) -> DynamicImage {
    let (width, height) = image.dimensions();
    if width == 0 || height == 0 {
        return image;
    }

    let rgb = image.to_rgb8();
    let level = |x, y| {
        let [red, green, blue] = rgb.get_pixel(x, y).0;
        red.max(green).max(blue)
    };
    let horizontal = matching_padding(width, height, level);
    let content_width = width - 2 * horizontal;
    let vertical = matching_padding(height, content_width, |y, x| level(x + horizontal, y));
    if horizontal == 0 && vertical == 0 {
        return image;
    }
    image.crop_imm(horizontal, vertical, content_width, height - 2 * vertical)
}

/// Find equal margins on one axis, with a sustained image edge just inside each margin.
fn matching_padding(length: u32, line_length: u32, level: impl Fn(u32, u32) -> u8) -> u32 {
    // Allow faint JPEG noise in a margin while rejecting bright details and dark gradients.
    let is_padding = |&position: &u32| {
        let (peak, sum) = (0..line_length)
            .map(|sample| level(position, sample))
            .fold((0, 0u64), |(peak, sum), value| {
                (peak.max(value), sum + u64::from(value))
            });
        peak <= 24 && sum <= 8 * u64::from(line_length)
    };
    let start = (0..length).take_while(is_padding).count() as u32;
    let end = (start..length).rev().take_while(is_padding).count() as u32;
    let content_length = length - start - end;
    let padding = start.min(end);
    if padding < length.div_ceil(20)
        || start.abs_diff(end) > length.div_ceil(50)
        || content_length < length.min(line_length).saturating_mul(2).div_ceil(3)
    {
        return 0;
    }

    // A lone label or unframed progress bar rarely has a long edge on both sides.
    let edge_depth = line_length.div_ceil(20).min(content_length / 2);
    let has_edge = |positions: std::ops::Range<u32>| {
        let samples = (0..line_length)
            .filter(|&sample| {
                positions
                    .clone()
                    .any(|position| level(position, sample) > 24)
            })
            .count() as u32;
        samples >= line_length.saturating_mul(2).div_ceil(3)
    };
    if has_edge(start..start + edge_depth) && has_edge(length - end - edge_depth..length - end) {
        padding
    } else {
        0
    }
}

/// Fit `image` inside `size` preserving aspect ratio, with black padding.
/// `set_button_image` stretches into the panel format, which distorts images
/// whose aspect differs (e.g. OpenDeck's 200x100 dial image on the 176x112
/// touch strip); pre-fitting here keeps shapes intact at the cost of small
/// letterbox bars.
fn fit_into(image: DynamicImage, size: (usize, usize), filter: FilterType) -> DynamicImage {
    let (ws, hs) = (size.0 as u32, size.1 as u32);
    let (w, h) = image.dimensions();
    if w == ws && h == hs {
        return image;
    }
    let scale = (ws as f32 / w as f32).min(hs as f32 / h as f32);
    let nw = ((w as f32 * scale).round() as u32).clamp(1, ws);
    let nh = ((h as f32 * scale).round() as u32).clamp(1, hs);
    let fitted = image.resize_exact(nw, nh, filter).into_rgb8();
    let mut canvas = RgbImage::new(ws, hs);
    image::imageops::replace(
        &mut canvas,
        &fitted,
        i64::from((ws - nw) / 2),
        i64::from((hs - nh) / 2),
    );
    DynamicImage::ImageRgb8(canvas)
}

#[cfg(test)]
mod tests {
    use super::*;
    use image::Rgb;

    #[test]
    fn full_strip_preserves_the_whole_dial_image() {
        let background = Rgb([0, 0, 255]);
        let edge = Rgb([255, 0, 0]);
        let image = RgbImage::from_fn(200, 100, |x, y| {
            if x == 0 || x == 199 || y == 0 || y == 99 {
                edge
            } else {
                background
            }
        });
        for (mode, height, top) in [
            (FullStrip::ImageSlot(3), 128, 14),
            (FullStrip::PanelBgpic, 130, 15),
        ] {
            let strip = StripCanvas::new(mode, true);
            let frame = strip
                .update(1, Some(DynamicImage::ImageRgb8(image.clone())))
                .unwrap()
                .into_rgb8();

            assert_eq!(frame.dimensions(), (800, height));
            assert_eq!(frame.view(200, top, 200, 100).to_image(), image);
            for (x, y) in [(200, top - 1), (399, top + 100), (199, 64), (400, 64)] {
                assert_eq!(frame.get_pixel(x, y), &Rgb([0, 0, 0]));
            }
        }
    }

    #[test]
    fn full_strip_scales_padded_square_to_fill_height() {
        // OpenDeck's fallback path centres an icon of up to 72x72 in its 200x100 canvas.
        let image = RgbImage::from_fn(200, 100, |x, y| {
            if (64..136).contains(&x) && (14..86).contains(&y) {
                Rgb([200, 100, 50])
            } else {
                Rgb([0, 0, 0])
            }
        });
        let mut jpeg = Vec::new();
        image::codecs::jpeg::JpegEncoder::new(&mut jpeg)
            .encode_image(&image)
            .unwrap();
        let image = load_from_memory_with_format(&jpeg, image::ImageFormat::Jpeg).unwrap();

        let strip = StripCanvas::new(FullStrip::ImageSlot(3), true);
        let frame = strip.update(0, Some(image)).unwrap().into_rgb8();

        // A side-only crop leaves the icon about 92 rows tall. Check near both edges.
        assert!(frame.get_pixel(100, 5)[0] > 128);
        assert!(frame.get_pixel(100, 122)[0] > 128);
    }

    #[test]
    fn auto_crop_preserves_progress_bar_lengths() {
        let strip = StripCanvas::new(FullStrip::PanelBgpic, true);
        for bar_width in [20, 80] {
            let bar = Rgb([200, 100, 50]);
            let image = RgbImage::from_fn(200, 100, |x, y| {
                if (100 - bar_width / 2..100 + bar_width / 2).contains(&x) && (42..58).contains(&y)
                {
                    bar
                } else {
                    Rgb([0, 0, 0])
                }
            });
            let frame = strip
                .update(0, Some(DynamicImage::ImageRgb8(image.clone())))
                .unwrap()
                .into_rgb8();
            assert_eq!(frame.view(0, 15, 200, 100).to_image(), image);
        }
    }

    #[test]
    fn disabling_auto_crop_preserves_padding() {
        let image = RgbImage::from_fn(200, 100, |x, _| {
            if (50..150).contains(&x) {
                Rgb([200, 100, 50])
            } else {
                Rgb([0, 0, 0])
            }
        });
        let strip = StripCanvas::new(FullStrip::PanelBgpic, false);
        let frame = strip
            .update(0, Some(DynamicImage::ImageRgb8(image.clone())))
            .unwrap()
            .into_rgb8();
        assert_eq!(frame.view(0, 15, 200, 100).to_image(), image);
    }
}
