use std::{collections::HashMap, sync::Arc, time::Duration};

use mirajazz::{device::list_devices, error::MirajazzError};
use openaction::device_plugin;
use tokio::{
    sync::Notify,
    task::JoinHandle,
    time::{MissedTickBehavior, interval, sleep, timeout},
};
use tokio_util::sync::CancellationToken;

use crate::{
    DEVICES,
    device::device_task,
    mappings::{CandidateDevice, DEVICE_NAMESPACE, Kind, QUERIES},
};

/// Includes connecting and stopping tasks, which must finish before an id is reused.
struct DeviceTask {
    cancel: CancellationToken,
    handle: JoinHandle<()>,
}

impl Drop for DeviceTask {
    fn drop(&mut self) {
        // Don't leave device I/O running if the watcher itself fails.
        self.cancel.cancel();
        self.handle.abort();
    }
}

async fn get_candidates() -> Result<Vec<CandidateDevice>, MirajazzError> {
    Ok(list_devices(QUERIES)
        .await?
        .into_iter()
        .filter_map(|dev| {
            let id = format!("{DEVICE_NAMESPACE}-{}", dev.serial_number.as_ref()?);
            let kind = Kind::from_vid_pid(dev.vendor_id, dev.product_id)?;
            Some(CandidateDevice {
                id,
                dev: dev.clone(),
                kind,
            })
        })
        .collect())
}

pub async fn watcher_task(shutdown: CancellationToken, resume: Arc<Notify>) {
    let mut tasks = HashMap::new();
    let mut rescan = interval(Duration::from_secs(5));
    rescan.set_missed_tick_behavior(MissedTickBehavior::Skip);
    loop {
        let candidates = tokio::select! {
            biased;
            _ = shutdown.cancelled() => break,
            _ = resume.notified() => {
                log::info!("Reconnecting devices after system resume");
                stop_devices(&mut tasks, true).await;
                // Give USB interfaces time to return. Polling retries if they're late.
                tokio::select! {
                    _ = shutdown.cancelled() => break,
                    _ = sleep(Duration::from_secs(1)) => {},
                }
                rescan.reset_immediately();
                continue;
            },
            result = async { rescan.tick().await; get_candidates().await } => match result {
                Ok(candidates) => candidates,
                Err(err) => {
                    log::warn!("Device enumeration failed: {err}; retrying");
                    continue;
                }
            },
        };
        // Cleanup is awaited outside cancellable futures, so an old task can
        // never outlive its entry and interfere with a replacement.
        reconcile_devices(&mut tasks, candidates).await;
    }
    stop_devices(&mut tasks, false).await;
}

async fn reconcile_devices(
    tasks: &mut HashMap<String, DeviceTask>,
    candidates: Vec<CandidateDevice>,
) {
    let stopped: Vec<_> = tasks
        .iter()
        .filter(|(id, task)| {
            task.handle.is_finished() || !candidates.iter().any(|candidate| &candidate.id == *id)
        })
        .map(|(id, _)| id.clone())
        .collect();
    // Cancel all removed devices before awaiting any of their shutdowns.
    for id in &stopped {
        tasks[id].cancel.cancel();
    }
    for id in stopped {
        finish_device(&id, tasks.remove(&id).unwrap()).await;
    }
    for candidate in candidates {
        tasks.entry(candidate.id.clone()).or_insert_with(|| {
            let cancel = CancellationToken::new();
            let handle = tokio::spawn(device_task(candidate, cancel.clone()));
            DeviceTask { cancel, handle }
        });
    }
}

async fn stop_devices(tasks: &mut HashMap<String, DeviceTask>, force: bool) {
    for task in tasks.values() {
        task.cancel.cancel();
        if force {
            // Resume must discard stale handles, even during pending initialization.
            task.handle.abort();
        }
    }
    for (id, task) in tasks.drain() {
        finish_device(&id, task).await;
    }
}

async fn finish_device(id: &str, mut task: DeviceTask) {
    if let Err(err) = (&mut task.handle).await
        && !err.is_cancelled()
    {
        log::warn!("Device {id} task failed: {err}");
    }
    if DEVICES.write().await.remove(id).is_some() {
        match timeout(
            Duration::from_secs(2),
            device_plugin::unregister_device(id.to_owned()),
        )
        .await
        {
            Ok(Ok(())) => {}
            Ok(Err(err)) => log::warn!("Failed to deregister device {id}: {err}"),
            Err(_) => log::warn!("Timed out deregistering device {id}"),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tokio::sync::oneshot;

    struct StaleHandle(Option<oneshot::Sender<()>>);

    impl Drop for StaleHandle {
        fn drop(&mut self) {
            self.0.take().unwrap().send(()).ok();
        }
    }

    #[tokio::test]
    async fn polling_removal_waits_for_graceful_device_exit() {
        let cancel = CancellationToken::new();
        let stopped = cancel.clone();
        let (finished, cleanup) = oneshot::channel();
        let handle = tokio::spawn(async move {
            stopped.cancelled().await;
            // A forced abort would drop this sender without completing cleanup.
            finished.send(()).unwrap();
        });
        let mut tasks =
            HashMap::from([("unplugged-device".to_owned(), DeviceTask { cancel, handle })]);

        timeout(
            Duration::from_secs(1),
            reconcile_devices(&mut tasks, Vec::new()),
        )
        .await
        .expect("polling must stop devices missing from enumeration");
        cleanup
            .await
            .expect("graceful cleanup must finish before removal");
        assert!(tasks.is_empty());
    }

    #[tokio::test]
    async fn resume_releases_stale_handles_even_when_device_task_is_stuck() {
        let cancel = CancellationToken::new();
        let (started, running) = oneshot::channel();
        let (released, dropped) = oneshot::channel();
        let handle = tokio::spawn(async move {
            let _handle = StaleHandle(Some(released));
            started.send(()).unwrap();
            std::future::pending::<()>().await;
        });
        let mut tasks = HashMap::from([(
            "stuck-before-resume".to_owned(),
            DeviceTask {
                cancel: cancel.clone(),
                handle,
            },
        )]);
        running.await.unwrap();

        timeout(Duration::from_secs(1), stop_devices(&mut tasks, true))
            .await
            .expect("resume must not wait for unresponsive device I/O");
        dropped
            .await
            .expect("stale handle must be released before reconnect");
        assert!(tasks.is_empty());
        assert!(cancel.is_cancelled());
    }
}
