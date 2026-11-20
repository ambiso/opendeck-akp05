//! Ambient rings consume the same samples as the four independent graph actions.
use crate::{desktop_monitor::Desktop, managed_device::ManagedDevice};
use std::{
    io,
    os::windows::io::{AsRawHandle, FromRawHandle, OwnedHandle},
    ptr,
    sync::atomic::{AtomicU64, Ordering},
    time::{Duration, SystemTime, UNIX_EPOCH},
};
use tokio::sync::watch;
use tokio_util::sync::CancellationToken;
use windows_sys::Win32::{
    Foundation::INVALID_HANDLE_VALUE,
    System::Memory::{
        CreateFileMappingW, FILE_MAP_READ, MapViewOfFile, PAGE_READWRITE, UnmapViewOfFile,
    },
};

struct Samples(OwnedHandle);
impl Samples {
    fn new(id: &str) -> io::Result<Self> {
        if id.contains(['\\', '\0']) {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "invalid device id for ambient rings",
            ));
        }
        let name: Vec<_> = format!(r"Local\Interstitial.Rings.v1.{id}")
            .encode_utf16()
            .chain([0])
            .collect();
        let h = unsafe {
            CreateFileMappingW(
                INVALID_HANDLE_VALUE,
                ptr::null(),
                PAGE_READWRITE,
                0,
                32,
                name.as_ptr(),
            )
        };
        if h.is_null() {
            Err(io::Error::last_os_error())
        } else {
            Ok(Self(unsafe { OwnedHandle::from_raw_handle(h) }))
        }
    }
    fn read(&self) -> [u64; 4] {
        unsafe {
            let v = MapViewOfFile(self.0.as_raw_handle(), FILE_MAP_READ, 0, 0, 32);
            if v.Value.is_null() {
                return [0; 4];
            }
            let a = std::array::from_fn(|i| {
                (&*v.Value.cast::<AtomicU64>().add(i)).load(Ordering::Acquire)
            });
            UnmapViewOfFile(v);
            a
        }
    }
}

type Color = [u8; 3];
const OFF: Color = [0, 0, 0];
const UNKNOWN: Color = [16, 16, 20];

#[derive(Default, Clone, Copy)]
struct Ring {
    kind: u8,
    step: i16,
}
impl Ring {
    fn color(&mut self, word: u64, now: u64) -> Color {
        let kind = (word >> 24) as u8;
        if !(1..=3).contains(&kind) || !now.checked_sub(word >> 32).is_some_and(|age| age <= 5) {
            self.kind = 0;
            return UNKNOWN;
        }
        let value = word as u16;
        if value > 1500 {
            self.kind = 0;
            return UNKNOWN;
        }
        // Thermal steps span 35..85 Celsius. Memory and network follow the strip scale.
        let (value, width, margin) = if kind == 1 {
            (i32::from(value) - 350, 50, 20)
        } else {
            (i32::from(value), 100, 20)
        };
        let old = i32::from(self.step);
        if kind != self.kind || value < old * width - margin || value >= (old + 1) * width + margin
        {
            self.step = (value.div_euclid(width)).clamp(0, 10) as i16;
            self.kind = kind;
        }
        let stops = match kind {
            1 => [[24, 98, 170], [42, 174, 181], [224, 157, 62], [232, 76, 48]],
            2 => [[12, 22, 40], [37, 83, 118], [70, 159, 182], [133, 219, 220]],
            _ => [[10, 21, 31], [19, 85, 88], [41, 151, 139], [102, 221, 175]],
        };
        let (a, b, t, n) = match self.step {
            s @ 0..=3 => (0, 1, s, 3),
            s @ 4..=7 => (1, 2, s - 3, 4),
            s => (2, 3, s - 7, 3),
        };
        std::array::from_fn(|i| ((stops[a][i] * (n - t) + stops[b][i] * t) / n) as u8)
    }
}
pub async fn run(
    id: &str,
    device: &ManagedDevice,
    mut desktop: watch::Receiver<Desktop>,
    cancel: CancellationToken,
    brightness: u8,
) {
    let samples = match Samples::new(id) {
        Ok(v) => v,
        Err(e) => {
            log::warn!("Ambient ring mapping unavailable: {e}");
            return;
        }
    };
    let mut power = device.power_changes();
    let mut rings = [Ring::default(); 4];
    let mut last = None;
    let mut failed = false;
    let mut epoch = None;
    loop {
        let awake = {
            let current = power.borrow_and_update();
            if current.has_changed() {
                last = None;
            }
            *current
        };
        let scene = *desktop.borrow_and_update();
        let locked = scene.locked;
        if epoch != Some(scene.epoch) {
            epoch = Some(scene.epoch);
            last = None;
        }
        let colors = if locked {
            [OFF; 4]
        } else {
            let words = samples.read();
            let now = SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap_or_default()
                .as_secs();
            std::array::from_fn(|i| rings[i].color(words[i], now))
        };
        if awake && (failed || last != Some(colors)) {
            let mut written = false;
            let result = device
                .with_awake_device(async |device| {
                    if desktop.borrow().locked != locked {
                        return Ok(());
                    }
                    if last.is_none() {
                        device.set_led_brightness(brightness).await?;
                    }
                    device.set_led_colors(&colors).await?;
                    written = true;
                    Ok(())
                })
                .await;
            match result {
                Ok(()) if !written => {}
                Ok(()) => {
                    if last.is_none()
                        || failed
                        || last == Some([UNKNOWN; 4])
                        || last == Some([OFF; 4])
                        || locked
                    {
                        log::info!("Ambient ring palette committed on {id}: {colors:?}");
                    }
                    last = Some(colors);
                    failed = false;
                }
                Err(e) => {
                    if !failed {
                        log::warn!("Ambient LED write failed: {e}");
                    }
                    failed = true;
                }
            }
        }
        tokio::select! {
            _=cancel.cancelled()=>break,
            event=power.changed()=>{ if event.is_err(){break} last=None; },
            event=desktop.changed()=>if event.is_err(){break},
            _=tokio::time::sleep(Duration::from_secs(1)),if awake && (!locked||failed)=>{}
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn pack(temp: u16) -> u64 {
        100 << 32 | 1 << 24 | u64::from(temp)
    }
    #[test]
    fn thermal_hysteresis_and_stale_samples() {
        let mut r = Ring::default();
        let cool = r.color(pack(640), 100);
        assert_eq!(cool, r.color(pack(651), 100));
        let warm = r.color(pack(671), 100);
        assert_ne!(warm, cool);
        assert_eq!(warm, r.color(pack(649), 100));
        assert_eq!(cool, r.color(pack(629), 100));
        assert_eq!(UNKNOWN, r.color(pack(880), 106));
        assert_eq!(UNKNOWN, r.color(0, 100));
    }
}
