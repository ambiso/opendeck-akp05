//! Windows VSD N4 Pro only. Use Mirabox's volatile JPEG framebuffer API;
//! the SDK's separate boot-logo functions write to ROM and are never loaded.
use crate::desktop_monitor::{Desktop, Monitor};
use crate::frame_jpeg::FrameEncoder;
use crate::managed_device::ManagedDevice;
use crate::mappings::CandidateDevice;
use async_hid::DeviceId;
use image::{ExtendedColorType, codecs::jpeg::JpegEncoder};
use libloading::os::windows::Library;
use std::{
    ffi::{CStr, c_char, c_void},
    ptr,
};

#[repr(C)]
struct HidInfo {
    path: *const c_char,
    vendor: u16,
    product: u16,
    serial: *const u16,
    release: u16,
    manufacturer: *const u16,
    name: *const u16,
    usage_page: u16,
    usage: u16,
    interface: i32,
    next: *mut HidInfo,
}

type Enumerate = unsafe extern "C" fn(u16, u16) -> *mut HidInfo;
type Free = unsafe extern "C" fn(*mut HidInfo);
type Open = unsafe extern "C" fn(*const HidInfo, *mut *mut c_void) -> u32;
type Close = unsafe extern "C" fn(*mut c_void) -> u32;
type Draw = unsafe extern "C" fn(*mut c_void, *const u8, usize, u16, u16, u16, u16, u8) -> u32;

// Keep the vendor's module resident until process exit, including reconnects.
static TRANSPORT: std::sync::LazyLock<Result<Library, String>> = std::sync::LazyLock::new(|| {
    let dll = std::env::current_exe()
        .map_err(|e| e.to_string())?
        .with_file_name("transport.dll");
    unsafe { Library::load_with_flags(dll, 0x100 | 0x800) }.map_err(|e| e.to_string())
});

use crate::strip::{Frames, compose_into};
use notify::{EventKind, RecommendedWatcher, RecursiveMode, Watcher};
use std::sync::atomic::{AtomicBool, AtomicI16, AtomicU64, Ordering};
use std::{
    collections::HashMap,
    sync::{Arc, LazyLock, Mutex},
    thread,
    time::Duration,
};
use tokio::sync::{oneshot, watch};

type Simple = unsafe extern "C" fn(*mut c_void) -> u32;
type KeyWrite = unsafe extern "C" fn(*mut c_void, *const u8, usize, u8) -> u32;
type Error = Box<dyn std::error::Error>;

type KeyCache = Mutex<HashMap<u8, Vec<u8>>>;
struct DisplayState {
    hidden: AtomicBool,
    keys: KeyCache,
    reset_count: AtomicU64,
    reset_wake: tokio::sync::Notify,
    cancel: tokio_util::sync::CancellationToken,
    offset: AtomicI16,
}
struct Panel {
    state: Arc<DisplayState>,
    uses_artwork: bool,
    desktop: watch::Receiver<Desktop>,
    _monitor: Monitor,
    _watcher: Option<RecommendedWatcher>,
    frames: Arc<watch::Sender<Frames>>,
    worker: thread::JoinHandle<()>,
}
static PANELS: LazyLock<Mutex<HashMap<String, Panel>>> = LazyLock::new(Default::default);

fn status(message: &str) {
    if let Some(dir) = dirs::config_dir() {
        let _ = std::fs::write(dir.join("Interstitial/strip-status.txt"), message);
    }
    log::info!("{message}");
}

pub async fn start(
    candidate: &CandidateDevice,
    y: u16,
    center_offset: i16,
    managed: Arc<ManagedDevice>,
    cancel: tokio_util::sync::CancellationToken,
) -> Result<(), String> {
    if !(-30..=30).contains(&center_offset) {
        return Err("strip_center_offset must be between -30 and 30".into());
    }
    if y > 320 {
        return Err("strip exceeds framebuffer".into());
    }
    let id = candidate.id.clone();
    let device = candidate.clone();
    let (frames, mut rx) = watch::channel(Frames::default());
    let frames = Arc::new(frames);
    let sources = Wallpapers::load();
    let uses_artwork = sources.night.is_some();
    let (monitor, desktop) = Monitor::start().map_err(|e| e.to_string())?;
    let mut worker_desktop = desktop.clone();
    let state = Arc::new(DisplayState {
        hidden: AtomicBool::new(uses_artwork && desktop.borrow().locked),
        keys: Mutex::default(),
        reset_count: AtomicU64::new(0),
        reset_wake: Default::default(),
        cancel,
        offset: AtomicI16::new(center_offset),
    });
    let watcher = alignment_watcher(&frames, &state)
        .map_err(|e| log::warn!("Watching strip alignment failed: {e}"))
        .ok();
    let mut display = DisplayWorker::new(state.clone(), sources, y, watcher.is_none())
        .map_err(|e| e.to_string())?;
    let (ready, started) = oneshot::channel();
    let rt = tokio::runtime::Handle::current();
    let worker = thread::spawn(move || {
        let session = match Session::open(&device) {
            Ok(s) => s,
            Err(e) => {
                let _ = ready.send(Err(e.to_string()));
                return;
            }
        };
        if ready.send(Ok(())).is_ok() {
            display.run(&session, &managed, &rt, &mut rx, &mut worker_desktop);
        }
    });
    PANELS.lock().unwrap().insert(
        id,
        Panel {
            frames,
            worker,
            uses_artwork,
            desktop,
            _monitor: monitor,
            state,
            _watcher: watcher,
        },
    );
    match started.await {
        Ok(Ok(())) => Ok(()),
        result => {
            stop(&candidate.id).await;
            Err(format!("Could not start panel worker: {result:?}"))
        }
    }
}

#[derive(Clone, Copy, PartialEq, Eq)]
struct Scene {
    light: bool,
    locked: bool,
    reset: u64,
    resume: u64,
}

struct DisplayWorker {
    state: Arc<DisplayState>,
    sources: Wallpapers,
    y: u16,
    poll_offset: bool,
    last_scene: Option<Scene>,
    reported_offset: Option<i16>,
    band: Option<image::RgbImage>,
    surface: image::RgbImage,
    encoder: FrameEncoder,
}
impl DisplayWorker {
    fn new(
        state: Arc<DisplayState>,
        sources: Wallpapers,
        y: u16,
        poll_offset: bool,
    ) -> turbojpeg::Result<Self> {
        Ok(Self {
            state,
            sources,
            y,
            poll_offset,
            last_scene: None,
            reported_offset: None,
            band: None,
            surface: image::RgbImage::new(800, 160),
            encoder: FrameEncoder::new()?,
        })
    }

    fn run(
        &mut self,
        session: &Session,
        managed: &ManagedDevice,
        rt: &tokio::runtime::Handle,
        rx: &mut watch::Receiver<Frames>,
        desktop: &mut watch::Receiver<Desktop>,
    ) {
        let mut changed = true;
        let mut power = managed.power_changes();
        loop {
            let awake = {
                let current = power.borrow_and_update();
                if current.has_changed() {
                    self.last_scene = None;
                }
                *current
            };
            if !awake {
                if !rt.block_on(async { tokio::select! { _ = self.state.cancel.cancelled() => false, r = power.changed() => r.is_ok() } }) { break; }
                self.last_scene = None;
                continue;
            }
            let current_desktop = *desktop.borrow_and_update();
            let scene = Scene {
                light: current_desktop.light,
                locked: self.sources.night.is_some() && current_desktop.locked,
                reset: self.state.reset_count.load(Ordering::Relaxed),
                resume: current_desktop.epoch,
            };
            let transition = self.last_scene != Some(scene);
            if transition && let Err(e) = self.change_scene(session, managed, scene) {
                status(&format!("Display transition failed: {e}"));
                if rt
                    .block_on(async {
                        tokio::time::timeout(Duration::from_secs(1), self.state.cancel.cancelled())
                            .await
                    })
                    .is_ok()
                {
                    break;
                }
                continue;
            }
            if draw_instruments(scene.locked, changed, transition) {
                let current = rx.borrow_and_update().clone();
                if let Err(e) = self.paint_strip(session, managed, &current) {
                    status(&format!("Joined frame failed: {e}"));
                    break;
                }
            }
            let wake = rt.block_on(async { tokio::select! {
                biased;
                _ = self.state.cancel.cancelled() => None,
                result = power.changed() => { self.last_scene = None; result.ok().map(|_| true) },
                wake = wait_for_display(rx, desktop, &self.state, scene.locked) => wake,
            }});
            let Some(has_frame) = wake else {
                break;
            };
            changed = has_frame;
        }
    }

    fn change_scene(
        &mut self,
        session: &Session,
        managed: &ManagedDevice,
        scene: Scene,
    ) -> Result<(), String> {
        let Some(_guard) = managed.awake_guard() else {
            return Ok(());
        };
        if scene.locked {
            self.state.hidden.store(true, Ordering::Release);
        }
        if let Some(art) = self.sources.pick(scene.light, scene.locked) {
            let native = image::imageops::rotate180(art);
            session.switch_scene(&native, scene.locked, &self.state.keys)?;
            self.band = Some(
                image::imageops::crop_imm(art, 0, 320 - u32::from(self.y), 800, 160).to_image(),
            );
        }
        self.state.hidden.store(scene.locked, Ordering::Release);
        let theme = if scene.light { "light" } else { "dark" };
        let session_state = if scene.locked { "locked" } else { "unlocked" };
        if let Some(path) = crate::config::config_path("desktop-state.txt") {
            let _ = std::fs::write(path, format!("{theme} {session_state}"));
        }
        self.last_scene = Some(scene);
        self.reported_offset = None;
        Ok(())
    }

    fn paint_strip(
        &mut self,
        session: &Session,
        managed: &ManagedDevice,
        frames: &Frames,
    ) -> Result<(), String> {
        if self.poll_offset
            && let Some(value) = crate::config::config_path("config.toml")
                .as_deref()
                .and_then(read_offset)
        {
            self.state.offset.store(value, Ordering::Relaxed);
        }
        let offset = self.state.offset.load(Ordering::Relaxed);
        compose_into(&mut self.surface, frames, offset, self.band.as_ref());
        let jpeg = self
            .encoder
            .encode(&self.surface)
            .map_err(|e| e.to_string())?;
        {
            let Some(_guard) = managed.awake_guard() else {
                return Ok(());
            };
            session.present_jpeg(jpeg, 800, 160, self.y)?;
        }
        if self.reported_offset != Some(offset) && frames.iter().all(Option::is_some) {
            status(&format!(
                "Committed all four live encoder images in a single 800x160 frame at (0,{}), content offset {offset}px.",
                self.y
            ));
            self.reported_offset = Some(offset);
        }
        Ok(())
    }
}

// Watch the directory because writers atomically replace the file. The weak
// sender lets unplug/shutdown finish even if a notification is in flight.
fn read_offset(path: &std::path::Path) -> Option<i16> {
    let cfg: crate::config::Config = toml::from_str(&std::fs::read_to_string(path).ok()?).ok()?;
    (-30..=30)
        .contains(&cfg.strip_center_offset)
        .then_some(cfg.strip_center_offset)
}
fn alignment_watcher(
    frames: &Arc<watch::Sender<Frames>>,
    state: &Arc<DisplayState>,
) -> Result<RecommendedWatcher, String> {
    let path =
        crate::config::config_path("config.toml").ok_or("Missing configuration directory")?;
    let watched = path.clone();
    let shared = state.clone();
    let wake = Arc::downgrade(frames);
    let mut watcher = notify::recommended_watcher(move |event: notify::Result<notify::Event>| {
        let Ok(event) = event else { return };
        if matches!(event.kind, EventKind::Access(_)) || !event.paths.contains(&watched) {
            return;
        }
        if let Some(value) = read_offset(&watched)
            && shared.offset.swap(value, Ordering::Relaxed) != value
            && let Some(sender) = wake.upgrade()
        {
            sender.send_modify(|_| {});
        }
    })
    .map_err(|e| e.to_string())?;
    watcher
        .watch(
            path.parent().ok_or("Invalid configuration directory")?,
            RecursiveMode::NonRecursive,
        )
        .map_err(|e| e.to_string())?;
    if let Some(value) = read_offset(&path) {
        state.offset.store(value, Ordering::Relaxed);
    }
    Ok(watcher)
}

pub fn update(id: &str, slot: u8, image: Option<&image::DynamicImage>) -> bool {
    let panels = PANELS.lock().unwrap();
    let Some(panel) = panels.get(id) else {
        return false;
    };
    if slot >= 4 || panel.frames.receiver_count() == 0 {
        return false;
    }
    let tile = image.map(|v| {
        Arc::new(if panel.uses_artwork {
            if v.width() == 200 && v.height() == 100 {
                v.to_rgb8()
            } else {
                v.resize_exact(200, 100, image::imageops::FilterType::Triangle)
                    .into_rgb8()
            }
        } else {
            crate::strip::full_height(v, (200, 160)).into_rgb8()
        })
    });
    panel
        .frames
        .send_modify(|frames| frames[slot as usize] = tile);
    true
}

async fn wait_for_display(
    rx: &mut watch::Receiver<Frames>,
    desktop: &mut watch::Receiver<Desktop>,
    state: &DisplayState,
    hidden: bool,
) -> Option<bool> {
    tokio::select! {
        biased;
        _ = state.cancel.cancelled() => None,
        change = desktop.changed() => change.ok().map(|_| false),
        _ = state.reset_wake.notified() => Some(false),
        frame = async {
            rx.changed().await.ok()?;
            tokio::time::sleep(Duration::from_millis(35)).await;
            Some(true)
        }, if !hidden => frame,
    }
}

fn draw_instruments(locked: bool, new_frame: bool, scene_changed: bool) -> bool {
    !locked && (new_frame || scene_changed)
}

struct Wallpapers {
    night: Option<image::RgbImage>,
    day: Option<image::RgbImage>,
    night_lock: Option<image::RgbImage>,
}
impl Wallpapers {
    fn load() -> Self {
        let load = |name| {
            crate::config::config_path(name)
                .and_then(|p| image::open(p).ok())
                .filter(|im| im.width() == 800 && im.height() == 480)
                .map(|im| im.into_rgb8())
        };
        Self {
            night: load("live-wallpaper.png"),
            day: load("day-wallpaper.png"),
            night_lock: load("night-original.png"),
        }
    }
    fn pick(&self, light: bool, locked: bool) -> Option<&image::RgbImage> {
        if light && let Some(day) = &self.day {
            return Some(day);
        }
        if locked && let Some(night) = &self.night_lock {
            return Some(night);
        }
        self.night.as_ref()
    }
}

pub(crate) fn desktop(id: &str) -> Option<watch::Receiver<Desktop>> {
    PANELS.lock().unwrap().get(id).map(|p| p.desktop.clone())
}
fn overlays_hidden(panel: &Panel) -> bool {
    (panel.uses_artwork && panel.desktop.borrow().locked)
        || panel.state.hidden.load(Ordering::Acquire)
}
pub fn hidden(id: &str) -> bool {
    PANELS.lock().unwrap().get(id).is_some_and(overlays_hidden)
}
pub fn cache_key(id: &str, slot: u8, bytes: Option<&[u8]>) -> bool {
    let panels = PANELS.lock().unwrap();
    let Some(panel) = panels.get(id) else {
        return false;
    };
    let mut keys = panel.state.keys.lock().unwrap();
    if let Some(bytes) = bytes {
        keys.insert(slot, bytes.to_vec());
    } else {
        keys.remove(&slot);
    }
    overlays_hidden(panel)
}
pub fn reset(id: &str) {
    if let Some(panel) = PANELS.lock().unwrap().get(id) {
        panel.state.keys.lock().unwrap().clear();
        panel.state.reset_count.fetch_add(1, Ordering::Relaxed);
        panel.frames.send_replace(Frames::default());
        panel.state.reset_wake.notify_one();
    }
}
pub async fn stop(id: &str) {
    let panel = PANELS.lock().unwrap().remove(id);
    if let Some(Panel {
        frames,
        worker,
        _watcher: watcher,
        _monitor: monitor,
        state,
        ..
    }) = panel
    {
        state.cancel.cancel();
        drop(watcher);
        drop(frames);
        let _ = tokio::task::spawn_blocking(move || {
            drop(monitor);
            worker.join()
        })
        .await;
    }
}

// Created, used and destroyed on one OS thread. The SDK enumeration and DLL
// outlive the handle; Drop closes the handle before freeing its device info.
struct Session {
    list: *mut HidInfo,
    handle: *mut c_void,
    free: Free,
    close: Close,
    send: Draw,
    refresh: Simple,
    clear_keys: Simple,
    write_key: KeyWrite,
}
impl Session {
    fn open(candidate: &CandidateDevice) -> Result<Self, Error> {
        let DeviceId::UncPath(path) = &candidate.dev.id else {
            return Err("missing Windows HID path".into());
        };
        let path = path.to_string_lossy();
        let lib = TRANSPORT.as_ref().map_err(|s| s.as_str())?;
        unsafe {
            let enumerate = *lib.get::<Enumerate>(b"transport_hid_enumerate\0")?;
            let free = *lib.get::<Free>(b"transport_hid_free_enumeration\0")?;
            let open = *lib.get::<Open>(b"transport_create\0")?;
            let mut s = Self {
                close: *lib.get::<Close>(b"transport_destroy\0")?,
                send: *lib.get::<Draw>(b"transport_set_background_frame_stream\0")?,
                refresh: *lib.get::<Simple>(b"transport_refresh\0")?,
                clear_keys: *lib.get::<Simple>(b"transport_clear_all_keys\0")?,
                write_key: *lib.get::<KeyWrite>(b"transport_set_key_image_stream\0")?,
                handle: ptr::null_mut(),
                free,
                list: enumerate(candidate.dev.vendor_id, candidate.dev.product_id),
            };
            let mut entry = s.list;
            while let Some(info) = entry.as_ref() {
                if !info.path.is_null()
                    && CStr::from_ptr(info.path)
                        .to_bytes()
                        .eq_ignore_ascii_case(path.as_bytes())
                {
                    let mut handle = ptr::null_mut();
                    let code = open(entry, &mut handle);
                    if code != 0 || handle.is_null() {
                        return Err(format!("SDK open {code:#x}").into());
                    }
                    s.handle = handle;
                    return Ok(s);
                }
                entry = info.next;
            }
        }
        Err("SDK could not match connected HID path".into())
    }
    fn switch_scene(
        &self,
        art: &image::RgbImage,
        locked: bool,
        keys: &KeyCache,
    ) -> Result<(), String> {
        let cleared = unsafe { (self.clear_keys)(self.handle) };
        if cleared != 0 {
            return Err(format!("clear keys {cleared:#x}"));
        }
        self.present(art, 0)?;
        if !locked {
            for (&index, bytes) in keys.lock().unwrap().iter() {
                let result = unsafe {
                    (self.write_key)(self.handle, bytes.as_ptr(), bytes.len(), index + 1)
                };
                if result != 0 {
                    return Err(format!("restore key {index}: {result:#x}"));
                }
            }
            let result = unsafe { (self.refresh)(self.handle) };
            if result != 0 {
                return Err(format!("restore refresh {result:#x}"));
            }
        }
        Ok(())
    }

    fn present(&self, img: &image::RgbImage, y: u16) -> Result<(), String> {
        let mut jpeg = Vec::new();
        JpegEncoder::new_with_quality(&mut jpeg, 90)
            .encode(
                img.as_raw(),
                img.width(),
                img.height(),
                ExtendedColorType::Rgb8,
            )
            .map_err(|e| e.to_string())?;
        self.present_jpeg(&jpeg, img.width() as u16, img.height() as u16, y)
    }
    fn present_jpeg(&self, jpeg: &[u8], width: u16, height: u16, y: u16) -> Result<(), String> {
        let sent = unsafe {
            (self.send)(
                self.handle,
                jpeg.as_ptr(),
                jpeg.len(),
                width,
                height,
                0,
                y,
                0,
            )
        };
        if sent != 0 {
            return Err(format!("framebuffer {sent:#x}"));
        }
        let committed = unsafe { (self.refresh)(self.handle) };
        if committed != 0 {
            return Err(format!("refresh {committed:#x}"));
        }
        Ok(())
    }
}
impl Drop for Session {
    fn drop(&mut self) {
        unsafe {
            if !self.handle.is_null() {
                (self.close)(self.handle);
            }
            if !self.list.is_null() {
                (self.free)(self.list);
            }
        }
    }
}
#[cfg(test)]
mod appearance_tests {
    use super::*;
    #[tokio::test]
    async fn locked_wait_ignores_frames_and_unlock_reads_latest() {
        let (frames, mut rx) = watch::channel(Frames::default());
        let (events, mut desktop) = watch::channel(Desktop {
            light: false,
            locked: true,
            epoch: 0,
        });
        let state = DisplayState {
            hidden: true.into(),
            keys: Default::default(),
            reset_count: 0.into(),
            offset: 0.into(),
            cancel: Default::default(),
            reset_wake: Default::default(),
        };
        let mut wait = Box::pin(wait_for_display(&mut rx, &mut desktop, &state, true));
        for n in 0..100u8 {
            frames.send_modify(|f| {
                f[0] = Some(Arc::new(image::RgbImage::from_pixel(
                    1,
                    1,
                    image::Rgb([n, 0, 0]),
                )))
            });
            assert!(futures_lite::future::poll_once(&mut wait).await.is_none());
        }
        events.send_modify(|s| s.locked = false);
        assert_eq!(wait.await, Some(false));
        assert_eq!(rx.borrow_and_update()[0].as_ref().unwrap()[(0, 0)][0], 99);
        state.cancel.cancel();
        assert_eq!(
            wait_for_display(&mut rx, &mut desktop, &state, true).await,
            None
        );
    }

    #[test]
    fn lock_hides_every_overlay_and_uses_the_uncropped_source_for_both_themes() {
        let art = |r| Some(image::RgbImage::from_pixel(1, 1, image::Rgb([r, 0, 0])));
        let pictures = Wallpapers {
            night: art(1),
            day: art(2),
            night_lock: art(3),
        };
        assert_eq!(pictures.pick(false, false).unwrap().get_pixel(0, 0)[0], 1);
        assert_eq!(pictures.pick(false, true).unwrap().get_pixel(0, 0)[0], 3);
        assert_eq!(pictures.pick(true, true).unwrap().get_pixel(0, 0)[0], 2);
        for changed in [true, false] {
            for transition in [true, false] {
                assert!(!draw_instruments(true, changed, transition));
            }
        }
        assert!(draw_instruments(false, false, true));
        assert!(!draw_instruments(false, false, false));
    }
}
