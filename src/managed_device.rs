use std::sync::Arc;

use mirajazz::{device::Device, error::MirajazzError, state::DeviceStateReader};
use tokio::sync::Mutex;

#[derive(Clone, Copy, PartialEq, Eq)]
enum PowerState {
    Awake,
    Sleeping,
}

struct ManagedDeviceInner<D> {
    device: D,
    power: PowerState,
}

/// Serializes power transitions with commands that could wake the device.
pub struct ManagedDevice<D = Device> {
    inner: Mutex<ManagedDeviceInner<D>>,
}

impl<D> ManagedDevice<D> {
    pub fn new(device: D) -> Self {
        Self {
            inner: Mutex::new(ManagedDeviceInner {
                device,
                power: PowerState::Awake,
            }),
        }
    }

    pub async fn with_awake_device(
        &self,
        operation: impl for<'a> AsyncFnOnce(&'a D) -> Result<(), MirajazzError>,
    ) -> Result<(), MirajazzError> {
        let inner = self.inner.lock().await;
        if inner.power == PowerState::Awake {
            operation(&inner.device).await?;
        }
        Ok(())
    }

    async fn set_power(
        &self,
        power: PowerState,
        operation: impl for<'a> AsyncFnOnce(&'a D) -> Result<(), MirajazzError>,
    ) -> Result<(), MirajazzError> {
        let mut inner = self.inner.lock().await;
        operation(&inner.device).await?;
        inner.power = power;
        Ok(())
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

    pub async fn get_reader(&self) -> Arc<DeviceStateReader> {
        self.inner
            .lock()
            .await
            .device
            .get_reader(crate::inputs::process_input)
    }

    pub async fn shutdown(&self) -> Result<(), MirajazzError> {
        self.inner.lock().await.device.shutdown().await
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use futures_lite::future::poll_once;
    use std::pin::pin;
    use tokio::sync::oneshot;

    #[tokio::test]
    async fn sleep_waits_for_inflight_command_and_blocks_queued_commands() {
        let device = ManagedDevice::new(Mutex::new(Vec::new()));
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
            *device.inner.lock().await.device.lock().await,
            ["keepalive started", "keepalive finished", "sleep"]
        );
    }
}
