use device::{handle_error, handle_set_image};
use managed_device::ManagedDevice;
use openaction::{
    OpenActionResult, async_trait,
    global_events::{SetBrightnessEvent, SetImageEvent, set_global_event_handler},
};
use std::{
    collections::HashMap,
    process::exit,
    sync::{Arc, LazyLock},
};
use tokio::sync::{Mutex, RwLock};
use tokio_util::{sync::CancellationToken, task::TaskTracker};
use watcher::watcher_task;

#[cfg(not(target_os = "windows"))]
use tokio::signal::unix::{SignalKind, signal};

mod config;
mod device;
mod inputs;
mod led_config;
mod managed_device;
mod mappings;
mod watcher;

pub static DEVICES: LazyLock<RwLock<HashMap<String, Arc<ManagedDevice>>>> =
    LazyLock::new(|| RwLock::new(HashMap::new()));
pub static TOKENS: LazyLock<RwLock<HashMap<String, CancellationToken>>> =
    LazyLock::new(|| RwLock::new(HashMap::new()));
pub static TRACKER: LazyLock<Mutex<TaskTracker>> = LazyLock::new(|| Mutex::new(TaskTracker::new()));

struct GlobalEventHandler {}
#[async_trait]
impl openaction::global_events::GlobalEventHandler for GlobalEventHandler {
    async fn plugin_ready(&self) -> OpenActionResult<()> {
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

    async fn device_plugin_set_image(&self, event: SetImageEvent) -> OpenActionResult<()> {
        log::debug!("Asked to set image");
        log::trace!("Set image event: {:#?}", event);

        let id = event.device.clone();

        let device = DEVICES.read().await.get(&event.device).cloned();
        if let Some(device) = device {
            if let Err(err) = device
                .with_awake_device(async |device| handle_set_image(device, event).await)
                .await
            {
                handle_error(&id, err).await;
            }
        } else {
            log::error!("Received event for unknown device: {}", event.device);
        }

        Ok(())
    }

    async fn device_plugin_set_brightness(
        &self,
        event: SetBrightnessEvent,
    ) -> OpenActionResult<()> {
        log::debug!("Asked to set brightness: {:#?}", event);

        let id = event.device.clone();

        let device = DEVICES.read().await.get(&event.device).cloned();
        if let Some(device) = device {
            if let Err(err) = device.set_brightness(event.brightness).await {
                handle_error(&id, err).await;
            }
        } else {
            log::error!("Received event for unknown device: {}", event.device);
        }

        Ok(())
    }
}

async fn shutdown() {
    let tokens = TOKENS.write().await;

    for token in tokens.values() {
        token.cancel();
    }
}

async fn connect() {
    static HANDLER: GlobalEventHandler = GlobalEventHandler {};
    set_global_event_handler(&HANDLER);
    if let Err(error) = openaction::run(std::env::args().collect()).await {
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
