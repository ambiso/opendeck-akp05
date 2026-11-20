use windows_sys::Win32::System::RemoteDesktop::{
    WTS_CURRENT_SESSION, WTS_SESSIONSTATE_LOCK, WTS_SESSIONSTATE_UNLOCK, WTSFreeMemory, WTSINFOEXW,
    WTSQuerySessionInformationW, WTSSessionInfoEx,
};
use winreg::{RegKey, enums::HKEY_CURRENT_USER};

pub fn light_theme() -> bool {
    RegKey::predef(HKEY_CURRENT_USER)
        .open_subkey(r"Software\Microsoft\Windows\CurrentVersion\Themes\Personalize")
        .and_then(|key| key.get_value::<u32, _>("AppsUseLightTheme"))
        .unwrap_or(0)
        != 0
}

// Ask about this process's own interactive session. On a failed/unknown query the
// display worker keeps overlays hidden until Windows explicitly reports unlocked.
pub fn locked() -> Option<bool> {
    let mut ptr = std::ptr::null_mut();
    let mut len = 0;
    unsafe {
        if WTSQuerySessionInformationW(
            std::ptr::null_mut(),
            WTS_CURRENT_SESSION,
            WTSSessionInfoEx,
            &mut ptr,
            &mut len,
        ) == 0
            || ptr.is_null()
        {
            return None;
        }
        let flags = if len as usize >= std::mem::size_of::<WTSINFOEXW>() {
            let info = &*ptr.cast::<WTSINFOEXW>();
            if info.Level == 1 {
                Some(info.Data.WTSInfoExLevel1.SessionFlags as u32)
            } else {
                None
            }
        } else {
            None
        };
        WTSFreeMemory(ptr.cast());
        match flags {
            Some(WTS_SESSIONSTATE_LOCK) => Some(true),
            Some(WTS_SESSIONSTATE_UNLOCK) => Some(false),
            _ => None,
        }
    }
}
