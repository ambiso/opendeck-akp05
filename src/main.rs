use device::handle_set_image;
use managed_device::ManagedDevice;
use openaction::{
    OpenActionResult, async_trait,
    global_events::{
        SetBrightnessEvent, SetImageEvent, SystemDidWakeUpEvent, set_global_event_handler,
    },
};
use std::{
    collections::HashMap,
    sync::{Arc, LazyLock},
};
use tokio::{
    sync::{Mutex, Notify, RwLock},
    task::JoinHandle,
};
use tokio_util::sync::CancellationToken;
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
static HANDLER: LazyLock<GlobalEventHandler> = LazyLock::new(|| GlobalEventHandler {
    shutdown: CancellationToken::new(),
    resume: Arc::new(Notify::new()),
    watcher: Mutex::new(None),
});

struct GlobalEventHandler {
    shutdown: CancellationToken,
    resume: Arc<Notify>,
    watcher: Mutex<Option<JoinHandle<()>>>,
}
#[async_trait]
impl openaction::global_events::GlobalEventHandler for GlobalEventHandler {
    async fn system_did_wake_up(&self, _event: SystemDidWakeUpEvent) -> OpenActionResult<()> {
        log::info!("System resumed; scheduling USB device rediscovery");
        self.resume.notify_one();
        Ok(())
    }

    async fn plugin_ready(&self) -> OpenActionResult<()> {
        let mut watcher = self.watcher.lock().await;
        if watcher.is_none() {
            *watcher = Some(tokio::spawn(watcher_task(
                self.shutdown.clone(),
                self.resume.clone(),
            )));
        }

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
                device.handle_error(&id, err);
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
                device.handle_error(&id, err);
            }
        } else {
            log::error!("Received event for unknown device: {}", event.device);
        }

        Ok(())
    }
}

async fn shutdown() {
    HANDLER.shutdown.cancel();
    if let Some(watcher) = HANDLER.watcher.lock().await.take()
        && let Err(err) = watcher.await
    {
        log::error!("Device watcher failed: {err}");
    }
}

async fn connect() -> OpenActionResult<()> {
    set_global_event_handler(&*HANDLER);
    openaction::run(std::env::args().collect()).await
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

    let result = tokio::select! {
        result = connect() => result,
        _ = sigterm() => Ok(()),
    };

    log::info!("Shutting down");

    shutdown().await;

    log::info!("Tasks are finished, exiting now");

    result.map_err(Into::into)
}
