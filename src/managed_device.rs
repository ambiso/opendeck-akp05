use std::sync::Arc;

use mirajazz::{device::Device, error::MirajazzError, state::DeviceStateReader};
use tokio::sync::Mutex;
use tokio_util::sync::CancellationToken;

#[derive(Clone, Copy, PartialEq, Eq)]
enum PowerState {
    Awake,
    Sleeping,
}

/// Serializes power transitions with commands that could wake the device.
pub struct ManagedDevice<D = Device> {
    device: D,
    power: Mutex<PowerState>,
    cancel: CancellationToken,
}

impl<D> ManagedDevice<D> {
    pub fn new(device: D, cancel: CancellationToken) -> Self {
        Self {
            device,
            power: Mutex::new(PowerState::Awake),
            cancel,
        }
    }

    pub async fn with_awake_device(
        &self,
        operation: impl for<'a> AsyncFnOnce(&'a D) -> Result<(), MirajazzError>,
    ) -> Result<(), MirajazzError> {
        tokio::select! {
            biased;
            _ = self.cancel.cancelled() => Ok(()),
            result = async {
                let power = self.power.lock().await;
                if *power == PowerState::Awake {
                    operation(&self.device).await?;
                }
                Ok(())
            } => result,
        }
    }

    async fn set_power(
        &self,
        power: PowerState,
        operation: impl for<'a> AsyncFnOnce(&'a D) -> Result<(), MirajazzError>,
    ) -> Result<(), MirajazzError> {
        tokio::select! {
            biased;
            _ = self.cancel.cancelled() => Ok(()),
            result = async {
                let mut current = self.power.lock().await;
                operation(&self.device).await?;
                *current = power;
                Ok(())
            } => result,
        }
    }

    /// Reports an operation error, cancelling this device on fatal errors.
    pub fn handle_error(&self, id: &str, err: MirajazzError) -> bool {
        log::error!("Device {id} error: {err}");
        if matches!(err, MirajazzError::ImageError(_) | MirajazzError::BadData) {
            return true;
        }
        self.cancel.cancel();
        false
    }
}

impl ManagedDevice {
    pub async fn set_brightness(&self, brightness: u8) -> Result<(), MirajazzError> {
        // LIG 0 leaves these panels lit. HAN sleeps them; nonzero LIG wakes them.
        if brightness == 0 {
            self.set_power(PowerState::Sleeping, async |device| device.sleep().await)
                .await
        } else {
            self.set_power(PowerState::Awake, async |device| {
                device.set_brightness(brightness).await
            })
            .await
        }
    }

    pub fn get_reader(&self) -> Arc<DeviceStateReader> {
        self.device.get_reader(crate::inputs::process_input)
    }

    pub async fn shutdown(&self) -> Result<(), MirajazzError> {
        let _power = self.power.lock().await;
        self.device.shutdown().await
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use futures_lite::future::poll_once;
    use std::pin::pin;
    use tokio::sync::oneshot;

    #[tokio::test]
    async fn cancellation_releases_inflight_and_queued_commands() {
        let cancel = CancellationToken::new();
        let device = ManagedDevice::new((), cancel.clone());
        let mut inflight = pin!(device.with_awake_device(async |_| {
            std::future::pending::<Result<(), MirajazzError>>().await
        }));
        assert!(poll_once(inflight.as_mut()).await.is_none());
        let mut queued = pin!(device.with_awake_device(async |_| {
            panic!("queued I/O must not start after cancellation");
        }));
        assert!(poll_once(queued.as_mut()).await.is_none());

        cancel.cancel();
        tokio::time::timeout(std::time::Duration::from_secs(1), async {
            inflight.await.unwrap();
            queued.await.unwrap();
        })
        .await
        .expect("dead HID I/O must not block event dispatch or shutdown");
        assert!(device.power.try_lock().is_ok());
    }

    #[tokio::test]
    async fn sleep_waits_for_inflight_command_and_blocks_queued_commands() {
        let device = ManagedDevice::new(Mutex::new(Vec::new()), CancellationToken::new());
        let (finish, finished) = oneshot::channel();
        let mut command = pin!(device.with_awake_device(async |events| {
            events.lock().await.push("keepalive started");
            finished.await.unwrap();
            events.lock().await.push("keepalive finished");
            Ok(())
        }));
        assert!(poll_once(command.as_mut()).await.is_none());

        let mut sleep = pin!(device.set_power(PowerState::Sleeping, async |events| {
            events.lock().await.push("sleep");
            Ok(())
        }));
        assert!(poll_once(sleep.as_mut()).await.is_none());
        let mut queued = pin!(device.with_awake_device(async |events| {
            events.lock().await.push("unexpected command");
            Ok(())
        }));
        assert!(poll_once(queued.as_mut()).await.is_none());

        finish.send(()).unwrap();
        command.await.unwrap();
        sleep.await.unwrap();
        queued.await.unwrap();
        assert_eq!(
            *device.device.lock().await,
            ["keepalive started", "keepalive finished", "sleep"]
        );
    }
}
