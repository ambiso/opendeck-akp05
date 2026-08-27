use device::{handle_error, handle_set_image};
use mappings::ENCODER_COUNT;
use mirajazz::device::Device;
use openaction::*;
use std::{collections::HashMap, process::exit, sync::LazyLock};
use tokio::sync::{Mutex, RwLock};
use tokio_util::{sync::CancellationToken, task::TaskTracker};
use watcher::watcher_task;

#[cfg(not(target_os = "windows"))]
use tokio::signal::unix::{SignalKind, signal};

mod device;
mod inputs;
mod led_config;
mod mappings;
mod watcher;

pub static DEVICES: LazyLock<RwLock<HashMap<String, Device>>> =
    LazyLock::new(|| RwLock::new(HashMap::new()));
pub static TOKENS: LazyLock<RwLock<HashMap<String, CancellationToken>>> =
    LazyLock::new(|| RwLock::new(HashMap::new()));
pub static TRACKER: LazyLock<Mutex<TaskTracker>> = LazyLock::new(|| Mutex::new(TaskTracker::new()));

struct GlobalEventHandler {}
impl openaction::GlobalEventHandler for GlobalEventHandler {
    async fn plugin_ready(
        &self,
        _outbound: &mut openaction::OutboundEventManager,
    ) -> EventHandlerResult {
        let tracker = TRACKER.lock().await.clone();

        let token = CancellationToken::new();
        tracker.spawn(watcher_task(token.clone()));

        TOKENS
            .write()
            .await
            .insert("_watcher_task".to_string(), token);

        log::info!("Plugin initialized");

        Ok(())
    }

    async fn set_image(
        &self,
        event: SetImageEvent,
        _outbound: &mut OutboundEventManager,
    ) -> EventHandlerResult {
        log::debug!("Asked to set image");
        log::trace!("Set image event: {:#?}", event);

        let id = event.device.clone();

        if let Some(device) = DEVICES.read().await.get(&event.device) {
            handle_set_image(device, event)
                .await
                .map_err(async |err| handle_error(&id, err).await)
                .ok();
        } else {
            log::error!("Received event for unknown device: {}", event.device);
        }

        Ok(())
    }

    async fn set_brightness(
        &self,
        event: SetBrightnessEvent,
        _outbound: &mut OutboundEventManager,
    ) -> EventHandlerResult {
        log::debug!("Asked to set brightness: {:#?}", event);

        let id = event.device.clone();

        if let Some(device) = DEVICES.read().await.get(&event.device) {
            device
                .set_brightness(event.brightness)
                .await
                .map_err(async |err| handle_error(&id, err).await)
                .ok();
        } else {
            log::error!("Received event for unknown device: {}", event.device);
        }

        Ok(())
    }
}

struct ActionEventHandler {}
impl openaction::ActionEventHandler for ActionEventHandler {}

async fn shutdown() {
    // Turn the hardware off before tearing the tasks down: otherwise the
    // device keeps displaying whatever it was last sent, for as long as USB
    // stays powered - which on many boards means until the PSU is switched
    // off.
    //
    // The encoder LEDs are a separate subsystem from the displays, so
    // shutdown() alone is not enough: it clears the screens but leaves the
    // rings lit. Clear them first — but only if we lit them in the first
    // place, since not every supported device has LED-equipped encoders.
    let had_leds = matches!(
        led_config::load().mode,
        Some(led_config::LedMode::Static { .. })
    );

    for (id, device) in DEVICES.read().await.iter() {
        if had_leds {
            if let Err(error) = device.set_led_colors(&[[0, 0, 0]; ENCODER_COUNT]).await {
                log::warn!("Failed to clear LED colors on {id}: {error}");
            }
            if let Err(error) = device.set_led_brightness(0).await {
                log::warn!("Failed to zero LED brightness on {id}: {error}");
            }
        }
        if let Err(error) = device.shutdown().await {
            log::warn!("Failed to shut down device {id}: {error}");
        }
    }

    let tokens = TOKENS.write().await;

    for (_, token) in tokens.iter() {
        token.cancel();
    }
}

async fn connect() {
    if let Err(error) = init_plugin(GlobalEventHandler {}, ActionEventHandler {}).await {
        log::error!("Failed to initialize plugin: {}", error);
        exit(1);
    }
}

#[cfg(any(target_os = "linux", target_os = "macos"))]
async fn sigterm() -> Result<(), Box<dyn std::error::Error>> {
    let mut sig = signal(SignalKind::terminate())?;

    sig.recv().await;

    Ok(())
}

#[cfg(target_os = "windows")]
async fn sigterm() -> Result<(), Box<dyn std::error::Error>> {
    // Future that would never resolve, so select only acts on OpenDeck connection loss
    // TODO: Proper windows termination handling
    std::future::pending::<()>().await;

    Ok(())
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    env_logger::Builder::from_env(env_logger::Env::default().default_filter_or("info")).init();

    tokio::select! {
        _ = connect() => {},
        _ = sigterm() => {},
    }

    log::info!("Shutting down");

    shutdown().await;

    let tracker = TRACKER.lock().await.clone();

    log::info!("Waiting for tasks to finish");

    tracker.close();
    tracker.wait().await;

    log::info!("Tasks are finished, exiting now");

    Ok(())
}
