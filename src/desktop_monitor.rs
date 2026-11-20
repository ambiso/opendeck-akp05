//! One notification window per active N4 panel. It never takes desktop focus.
use crate::desktop_state::{light_theme, locked};
use std::{
    io,
    os::windows::io::{AsRawHandle, FromRawHandle, OwnedHandle},
    ptr,
    sync::Arc,
    thread,
};
use tokio::sync::watch;
use windows_sys::{
    Win32::{
        Foundation::{
            HANDLE, HWND, LPARAM, LRESULT, WAIT_FAILED, WAIT_OBJECT_0, WAIT_TIMEOUT, WPARAM,
        },
        System::{
            LibraryLoader::GetModuleHandleW,
            Registry::{REG_NOTIFY_CHANGE_LAST_SET, RegNotifyChangeKeyValue},
            RemoteDesktop::{
                NOTIFY_FOR_THIS_SESSION, WTSRegisterSessionNotification,
                WTSUnRegisterSessionNotification,
            },
            Threading::{CreateEventW, INFINITE, SetEvent, WaitForSingleObject},
        },
        UI::WindowsAndMessaging::*,
    },
    core::w,
};
use winreg::{
    RegKey,
    enums::{HKEY_CURRENT_USER, KEY_NOTIFY},
};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Desktop {
    pub light: bool,
    pub locked: bool,
    pub epoch: u64,
}

fn sample(epoch: u64) -> Desktop {
    Desktop {
        light: light_theme(),
        locked: locked() != Some(false),
        epoch,
    }
}
fn publish(tx: &watch::Sender<Desktop>, next: Desktop) {
    tx.send_if_modified(|s| {
        if *s == next {
            false
        } else {
            *s = next;
            true
        }
    });
}
fn event(manual: bool) -> io::Result<OwnedHandle> {
    let h = unsafe { CreateEventW(ptr::null(), manual.into(), 0, ptr::null()) };
    if h.is_null() {
        Err(io::Error::last_os_error())
    } else {
        Ok(unsafe { OwnedHandle::from_raw_handle(h) })
    }
}

pub struct Monitor {
    stop: Arc<OwnedHandle>,
    worker: Option<thread::JoinHandle<()>>,
}
impl Monitor {
    pub fn start() -> io::Result<(Self, watch::Receiver<Desktop>)> {
        let stop = Arc::new(event(true)?);
        let quit = stop.clone();
        let (tx, rx) = watch::channel(sample(0));
        let worker=thread::Builder::new().name("N4 desktop notifications".into()).spawn(move|| {
            loop {
                match observe(quit.as_raw_handle(),&tx) {
                    Ok(())=>break,
                    Err(e)=>log::warn!("Desktop observation interrupted: {e}; using one-second fallback before resubscribing"),
                }
                for _ in 0..30 {
                    let epoch=tx.borrow().epoch;
                    publish(&tx,sample(epoch));
                    if unsafe{WaitForSingleObject(quit.as_raw_handle(),1000)}!=WAIT_TIMEOUT{return}
                }
            }
        })?;
        Ok((
            Self {
                stop,
                worker: Some(worker),
            },
            rx,
        ))
    }
}
impl Drop for Monitor {
    fn drop(&mut self) {
        unsafe {
            SetEvent(self.stop.as_raw_handle());
        }
        if let Some(t) = self.worker.take() {
            let _ = t.join();
        }
    }
}

const RESYNC: u32 = WM_APP + 21;
const LOCK: u32 = WM_APP + 22;
const RESUME: u32 = WM_APP + 23;
const UNLOCK: u32 = WM_APP + 24;
unsafe extern "system" fn wndproc(hwnd: HWND, msg: u32, w: WPARAM, l: LPARAM) -> LRESULT {
    let signal = match (msg, w as u32) {
        (WM_WTSSESSION_CHANGE, WTS_SESSION_LOCK) => LOCK,
        (WM_WTSSESSION_CHANGE, WTS_SESSION_UNLOCK) => UNLOCK,
        (WM_WTSSESSION_CHANGE, _) => RESYNC,
        (WM_POWERBROADCAST, PBT_APMRESUMEAUTOMATIC | PBT_APMRESUMECRITICAL) => RESUME,
        (WM_POWERBROADCAST, PBT_APMRESUMESUSPEND) => RESYNC,
        (WM_POWERBROADCAST, PBT_APMSUSPEND) => LOCK,
        _ => return unsafe { DefWindowProcW(hwnd, msg, w, l) },
    };
    unsafe {
        PostMessageW(hwnd, signal, 0, 0);
    }
    1
}

struct Window(HWND);
impl Window {
    fn open() -> io::Result<Self> {
        let instance = unsafe { GetModuleHandleW(ptr::null()) };
        let name = w!("Interstitial.N4.DesktopEvents.v1");
        let class = WNDCLASSW {
            lpfnWndProc: Some(wndproc),
            hInstance: instance,
            lpszClassName: name,
            ..Default::default()
        };
        if unsafe { RegisterClassW(&class) } == 0 {
            let err = io::Error::last_os_error();
            if err.raw_os_error()
                != Some(windows_sys::Win32::Foundation::ERROR_CLASS_ALREADY_EXISTS as i32)
            {
                return Err(err);
            }
        }
        let hwnd = unsafe {
            CreateWindowExW(
                WS_EX_NOACTIVATE | WS_EX_TOOLWINDOW,
                name,
                name,
                WS_POPUP,
                0,
                0,
                0,
                0,
                ptr::null_mut(),
                ptr::null_mut(),
                instance,
                ptr::null(),
            )
        };
        if hwnd.is_null() {
            return Err(io::Error::last_os_error());
        }
        if unsafe { WTSRegisterSessionNotification(hwnd, NOTIFY_FOR_THIS_SESSION) } == 0 {
            let e = io::Error::last_os_error();
            unsafe {
                DestroyWindow(hwnd);
            }
            return Err(e);
        }
        Ok(Self(hwnd))
    }
}
impl Drop for Window {
    fn drop(&mut self) {
        unsafe {
            WTSUnRegisterSessionNotification(self.0);
            DestroyWindow(self.0);
        }
    }
}

fn arm(key: &RegKey, h: HANDLE) -> io::Result<()> {
    let err = unsafe {
        RegNotifyChangeKeyValue(key.raw_handle() as _, 0, REG_NOTIFY_CHANGE_LAST_SET, h, 1)
    };
    if err == 0 {
        Ok(())
    } else {
        Err(io::Error::from_raw_os_error(err as i32))
    }
}
fn observe(stop: HANDLE, tx: &watch::Sender<Desktop>) -> io::Result<()> {
    let _window = Window::open()?;
    let key = RegKey::predef(HKEY_CURRENT_USER).open_subkey_with_flags(
        r"Software\Microsoft\Windows\CurrentVersion\Themes\Personalize",
        KEY_NOTIFY,
    )?;
    let theme = event(false)?;
    let handles = [stop, theme.as_raw_handle()];
    arm(&key, handles[1])?;
    let mut epoch = tx.borrow().epoch;
    let resample = |epoch| {
        let state = locked();
        publish(
            tx,
            Desktop {
                light: light_theme(),
                locked: state != Some(false),
                epoch,
            },
        );
        state.ok_or_else(|| io::Error::other("WTS session state unavailable"))
    };
    resample(epoch)?;
    log::info!("N4 desktop notifications active (WTS, resume and theme)");
    loop {
        let n = unsafe {
            MsgWaitForMultipleObjectsEx(
                2,
                handles.as_ptr(),
                INFINITE,
                QS_ALLINPUT,
                MWMO_INPUTAVAILABLE,
            )
        };
        match n {
            WAIT_OBJECT_0 => return Ok(()),
            1 => {
                arm(&key, handles[1])?;
                let mut s = *tx.borrow();
                s.light = light_theme();
                publish(tx, s);
            }
            2 => {
                let mut msg = MSG::default();
                while unsafe { PeekMessageW(&mut msg, ptr::null_mut(), 0, 0, PM_REMOVE) } != 0 {
                    match msg.message {
                        LOCK => {
                            let mut s = *tx.borrow();
                            s.locked = true;
                            publish(tx, s);
                        }
                        RESUME => {
                            epoch = epoch.wrapping_add(1);
                            resample(epoch)?;
                        }
                        UNLOCK => {
                            if resample(epoch)? {
                                return Err(io::Error::other("WTS unlock state not yet confirmed"));
                            }
                        }
                        RESYNC => {
                            resample(epoch)?;
                        }
                        _ => unsafe {
                            TranslateMessage(&msg);
                            DispatchMessageW(&msg);
                        },
                    }
                }
            }
            WAIT_FAILED => return Err(io::Error::last_os_error()),
            _ => return Err(io::Error::other("unexpected desktop wait result")),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    #[ignore = "requires an interactive Windows session with WTS"]
    fn notification_window_forwards_session_and_resume_events() {
        let window = Window::open().unwrap();
        for (message, param, expected) in [
            (WM_WTSSESSION_CHANGE, WTS_SESSION_LOCK, LOCK),
            (WM_WTSSESSION_CHANGE, WTS_SESSION_UNLOCK, UNLOCK),
            (WM_POWERBROADCAST, PBT_APMRESUMEAUTOMATIC, RESUME),
        ] {
            unsafe {
                SendMessageW(window.0, message, param as usize, 0);
                let mut msg = MSG::default();
                assert_ne!(
                    PeekMessageW(&mut msg, window.0, expected, expected, PM_REMOVE),
                    0
                );
            }
        }
    }
}
