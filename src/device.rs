use std::time::Duration;

use data_url::DataUrl;
use image::{DynamicImage, GenericImageView, load_from_memory_with_format};
use mirajazz::{device::Device, error::MirajazzError, state::DeviceStateUpdate};
use openaction::{OUTBOUND_EVENT_MANAGER, SetImageEvent};
use tokio::time::interval;
use tokio_util::sync::CancellationToken;

use crate::{
    DEVICES, TOKENS, config, led_config,
    mappings::{
        COL_COUNT, CandidateDevice, DEVICE_TYPE, ENCODER_COUNT, KEY_COUNT, Kind, ROW_COUNT,
    },
};

/// Initializes a device and listens for events
pub async fn device_task(candidate: CandidateDevice, token: CancellationToken) {
    log::info!("Running device task for {:?}", candidate);

    // Wrap in a closure so we can use `?` operator
    let device = async {
        let device = connect(&candidate).await?;

        device.set_brightness(50).await?;
        device.clear_all_button_images().await?;
        device.flush().await?;

        let cfg = config::load();
        log::info!("Applying config: {:?}", cfg);
        if let Some(led_config::LedMode::Static { colors: _ }) = cfg.leds.mode {
            device.set_led_brightness(cfg.leds.brightness).await?;
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

        Ok(device)
    }
    .await;

    let device: Device = match device {
        Ok(device) => device,
        Err(err) => {
            handle_error(&candidate.id, err).await;

            log::error!(
                "Had error during device init, finishing device task: {:?}",
                candidate
            );

            return;
        }
    };

    log::info!("Registering device {}", candidate.id);
    if let Some(outbound) = OUTBOUND_EVENT_MANAGER.lock().await.as_mut() {
        outbound
            .register_device(
                candidate.id.clone(),
                candidate.kind.human_name(),
                ROW_COUNT as u8,
                COL_COUNT as u8,
                ENCODER_COUNT as u8,
                DEVICE_TYPE,
            )
            .await
            .unwrap();
    }

    DEVICES.write().await.insert(candidate.id.clone(), device);

    tokio::select! {
        _ = device_events_task(&candidate) => {},
        _ = keepalive_task(&candidate) => {},
        _ = token.cancelled() => {}
    };

    log::info!("Shutting down device {:?}", candidate);

    if let Some(device) = DEVICES.read().await.get(&candidate.id) {
        device.shutdown().await.ok();
    }

    log::info!("Device task finished for {:?}", candidate);
}

/// Handles errors, returning true if should continue, returning false if an error is fatal
pub async fn handle_error(id: &String, err: MirajazzError) -> bool {
    log::error!("Device {} error: {}", id, err);

    // Some errors are not critical and can be ignored without sending disconnected event
    if matches!(err, MirajazzError::ImageError(_) | MirajazzError::BadData) {
        return true;
    }

    log::info!("Deregistering device {}", id);
    if let Some(outbound) = OUTBOUND_EVENT_MANAGER.lock().await.as_mut() {
        outbound.deregister_device(id.clone()).await.unwrap();
    }

    log::info!("Cancelling tasks for device {}", id);
    if let Some(token) = TOKENS.read().await.get(id) {
        token.cancel();
    }

    log::info!("Removing device {} from the list", id);
    DEVICES.write().await.remove(id);
    crate::SLEEPING.write().await.remove(id);

    log::info!("Finished clean-up for {}", id);

    false
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
async fn device_events_task(candidate: &CandidateDevice) -> Result<(), MirajazzError> {
    log::info!("Connecting to {} for incoming events", candidate.id);

    let devices_lock = DEVICES.read().await;
    let reader = match devices_lock.get(&candidate.id) {
        Some(device) => device.get_reader(crate::inputs::process_input),
        None => return Ok(()),
    };
    drop(devices_lock);

    log::info!("Connected to {} for incoming events", candidate.id);

    log::info!("Reader is ready for {}", candidate.id);

    loop {
        log::debug!("Reading updates...");

        let updates = match reader.read(None).await {
            Ok(updates) => updates,
            Err(e) => {
                if !handle_error(&candidate.id, e).await {
                    break;
                }

                continue;
            }
        };

        for update in updates {
            log::debug!("New update: {:#?}", update);

            let id = candidate.id.clone();

            if let Some(outbound) = OUTBOUND_EVENT_MANAGER.lock().await.as_mut() {
                match update {
                    DeviceStateUpdate::ButtonDown(key) => outbound.key_down(id, key).await.unwrap(),
                    DeviceStateUpdate::ButtonUp(key) => outbound.key_up(id, key).await.unwrap(),
                    DeviceStateUpdate::EncoderDown(encoder) => {
                        outbound.encoder_down(id, encoder).await.unwrap();
                    }
                    DeviceStateUpdate::EncoderUp(encoder) => {
                        outbound.encoder_up(id, encoder).await.unwrap();
                    }
                    DeviceStateUpdate::EncoderTwist(encoder, val) => {
                        outbound
                            .encoder_change(id, encoder, val as i16)
                            .await
                            .unwrap();
                    }
                }
            }
        }
    }

    Ok(())
}

/// Sends periodic keepalives to the device to maintain connection
async fn keepalive_task(candidate: &CandidateDevice) -> Result<(), MirajazzError> {
    let mut interval = interval(Duration::from_secs(10));

    loop {
        interval.tick().await;

        // While a device is asleep, the periodic CONNECT would wake its panels
        // back up, so skip it until something sets a non-zero brightness again.
        if crate::SLEEPING.read().await.contains(&candidate.id) {
            continue;
        }

        log::debug!("Sending keepalive to {}", candidate.id);

        let devices_lock = DEVICES.read().await;
        let device = match devices_lock.get(&candidate.id) {
            Some(device) => device,
            None => return Ok(()),
        };

        if let Err(e) = device.keep_alive().await {
            drop(devices_lock);
            if !handle_error(&candidate.id, e).await {
                break;
            }
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
pub async fn handle_set_image(device: &Device, evt: SetImageEvent) -> Result<(), MirajazzError> {
    let is_encoder = evt.controller.as_deref() == Some("Encoder");
    match (evt.position, evt.image) {
        (Some(position), Some(image)) => {
            log::debug!("Setting image for button {}", position);
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

            let kind = Kind::from_vid_pid(device.vid, device.pid).unwrap();
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
                fit_into(image, format.size)
            } else {
                image
            };

            device.set_button_image(position, format, image).await?;
            device.flush().await?;
        }
        (Some(position), None) => {
            let position = map_position(position, is_encoder)?;
            device.clear_button_image(position).await?;
            device.flush().await?;
        }
        (None, None) => {
            device.clear_all_button_images().await?;
            device.flush().await?;
        }
        _ => {}
    }

    Ok(())
}

/// Fit `image` inside `size` preserving aspect ratio, centered on black.
/// `set_button_image` stretches into the panel format, which distorts images
/// whose aspect differs (e.g. OpenDeck's 200x100 dial image on the 176x112
/// touch strip); pre-fitting here keeps shapes intact at the cost of small
/// letterbox bars.
fn fit_into(image: DynamicImage, size: (usize, usize)) -> DynamicImage {
    let (ws, hs) = (size.0 as u32, size.1 as u32);
    let (w, h) = image.dimensions();
    if w == ws && h == hs {
        return image;
    }
    let scale = (ws as f32 / w as f32).min(hs as f32 / h as f32);
    let nw = ((w as f32 * scale).round() as u32).clamp(1, ws);
    let nh = ((h as f32 * scale).round() as u32).clamp(1, hs);
    let fitted = image
        .resize_exact(nw, nh, image::imageops::FilterType::Nearest)
        .into_rgb8();
    let mut canvas = image::RgbImage::from_pixel(ws, hs, image::Rgb([0, 0, 0]));
    image::imageops::overlay(
        &mut canvas,
        &fitted,
        ((ws - nw) / 2) as i64,
        ((hs - nh) / 2) as i64,
    );
    DynamicImage::ImageRgb8(canvas)
}
