//! «Is Artem on a call?» — without touching the camera or the microphone.
//! Windows records which apps use the mic and the webcam under
//! `HKCU\Software\Microsoft\Windows\CurrentVersion\CapabilityAccessManager\ConsentStore\{microphone,webcam}`:
//! one subkey per packaged app (`MSTeams_8wekyb3d8bbwe`) plus `NonPackaged\<exe path with # for \>`,
//! each with `LastUsedTimeStart` / `LastUsedTimeStop` (FILETIME). A started, not yet stopped
//! entry means the device is in use right now. Our own processes (the app's window, Hermes'
//! ffmpeg and Python) are ignored, so always-listening mode does not count as a call.

use serde::{Deserialize, Serialize};
use ts_rs::TS;

#[derive(Debug, Clone, Default, Serialize, Deserialize, TS, PartialEq)]
#[ts(export, export_to = "../../src/types/generated/")]
pub struct CallStatus {
    /// Some other app holds the microphone or the camera.
    pub in_call: bool,
    /// Friendly names («Teams», «Telegram»), without duplicates.
    pub apps: Vec<String>,
}

/// One registry entry: subkey name, LastUsedTimeStart, LastUsedTimeStop.
pub type Usage = (String, u64, u64);

/// Processes that are ours: their mic/camera use is not a call.
const OURS: &[&str] = &[
    "hermes-assistant",
    "msedgewebview2",
    "ffmpeg",
    "python",
    "pythonw",
    "ollama",
];

/// `C:#Users#…#Telegram.exe` → «Telegram», `MSTeams_8wekyb3d8bbwe` → «Teams»,
/// `5319275A.WhatsAppDesktop_cv1g…` → «WhatsApp».
pub fn friendly_name(key: &str) -> String {
    if key.contains('#') {
        let file = key.rsplit('#').next().unwrap_or(key);
        let stem = file.strip_suffix(".exe").unwrap_or(file);
        return match stem.to_ascii_lowercase().as_str() {
            "ms-teams" | "teams" => "Teams".into(),
            "ts3client_win64" => "TeamSpeak".into(),
            "chrome" => "Chrome".into(),
            "msedge" => "Edge".into(),
            "zoom" => "Zoom".into(),
            _ => stem.to_string(),
        };
    }
    let family = key.split('_').next().unwrap_or(key);
    let name = family.rsplit('.').next().unwrap_or(family);
    let name = name
        .strip_prefix("MS")
        .filter(|n| !n.is_empty())
        .unwrap_or(name);
    name.strip_suffix("Desktop")
        .filter(|n| !n.is_empty())
        .unwrap_or(name)
        .to_string()
}

fn is_ours(key: &str) -> bool {
    let file = key
        .rsplit(['#', '\\'])
        .next()
        .unwrap_or(key)
        .to_ascii_lowercase();
    let stem = file.strip_suffix(".exe").unwrap_or(&file);
    OURS.contains(&stem)
}

/// Apps whose use started and has not stopped.
pub fn active(entries: &[Usage]) -> CallStatus {
    let mut apps: Vec<String> = Vec::new();
    for (key, start, stop) in entries {
        if *start == 0 || *stop != 0 || is_ours(key) {
            continue;
        }
        let name = friendly_name(key);
        if !apps.contains(&name) {
            apps.push(name);
        }
    }
    CallStatus {
        in_call: !apps.is_empty(),
        apps,
    }
}

/// Current status of the microphone and the webcam (empty off Windows).
pub fn status() -> CallStatus {
    let mut entries = Vec::new();
    for cap in ["microphone", "webcam"] {
        entries.extend(platform::read(cap));
    }
    active(&entries)
}

#[cfg(windows)]
mod platform {
    use super::Usage;
    use windows_sys::Win32::Foundation::ERROR_SUCCESS;
    use windows_sys::Win32::System::Registry::{
        HKEY, HKEY_CURRENT_USER, KEY_READ, RRF_RT_REG_QWORD, RegCloseKey, RegEnumKeyExW,
        RegGetValueW, RegOpenKeyExW,
    };

    fn wide(s: &str) -> Vec<u16> {
        s.encode_utf16().chain(std::iter::once(0)).collect()
    }

    fn qword(key: HKEY, sub: &[u16], name: &str) -> u64 {
        let mut value: u64 = 0;
        let mut size = std::mem::size_of::<u64>() as u32;
        let name = wide(name);
        // SAFETY: valid key handle, NUL-terminated strings, buffer of `size` bytes.
        let rc = unsafe {
            RegGetValueW(
                key,
                sub.as_ptr(),
                name.as_ptr(),
                RRF_RT_REG_QWORD,
                std::ptr::null_mut(),
                (&mut value as *mut u64).cast(),
                &mut size,
            )
        };
        if rc == ERROR_SUCCESS { value } else { 0 }
    }

    /// (subkey, start, stop) for every direct subkey of `path`.
    fn entries(path: &str) -> Vec<Usage> {
        let mut out = Vec::new();
        let mut key: HKEY = std::ptr::null_mut();
        let p = wide(path);
        // SAFETY: out-pointer to a local handle, closed below.
        if unsafe { RegOpenKeyExW(HKEY_CURRENT_USER, p.as_ptr(), 0, KEY_READ, &mut key) }
            != ERROR_SUCCESS
        {
            return out;
        }
        let mut i = 0u32;
        loop {
            let mut buf = [0u16; 512];
            let mut len = buf.len() as u32;
            // SAFETY: buffer of `len` UTF-16 units; other out-params unused.
            let rc = unsafe {
                RegEnumKeyExW(
                    key,
                    i,
                    buf.as_mut_ptr(),
                    &mut len,
                    std::ptr::null(),
                    std::ptr::null_mut(),
                    std::ptr::null_mut(),
                    std::ptr::null_mut(),
                )
            };
            if rc != ERROR_SUCCESS {
                break;
            }
            i += 1;
            let name = String::from_utf16_lossy(&buf[..len as usize]);
            let sub: Vec<u16> = buf[..len as usize].iter().copied().chain([0]).collect();
            out.push((
                name,
                qword(key, &sub, "LastUsedTimeStart"),
                qword(key, &sub, "LastUsedTimeStop"),
            ));
        }
        // SAFETY: handle opened above.
        unsafe { RegCloseKey(key) };
        out
    }

    pub fn read(capability: &str) -> Vec<Usage> {
        let root = format!(
            r"Software\Microsoft\Windows\CurrentVersion\CapabilityAccessManager\ConsentStore\{capability}"
        );
        let mut all: Vec<Usage> = entries(&root)
            .into_iter()
            .filter(|(k, _, _)| k != "NonPackaged")
            .collect();
        all.extend(entries(&format!(r"{root}\NonPackaged")));
        all
    }
}

#[cfg(not(windows))]
mod platform {
    pub fn read(_capability: &str) -> Vec<super::Usage> {
        Vec::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn names() {
        assert_eq!(friendly_name("MSTeams_8wekyb3d8bbwe"), "Teams");
        assert_eq!(
            friendly_name("5319275A.WhatsAppDesktop_cv1g1gvanyjgm"),
            "WhatsApp"
        );
        assert_eq!(
            friendly_name("C:#Users#Koava#AppData#Roaming#Telegram Desktop#Telegram.exe"),
            "Telegram"
        );
        assert_eq!(
            friendly_name("C:#Program Files#TeamSpeak 3 Client#ts3client_win64.exe"),
            "TeamSpeak"
        );
        assert_eq!(
            friendly_name("C:#Users#Koava#AppData#Local#Discord#app-1.0.9258#Discord.exe"),
            "Discord"
        );
    }

    #[test]
    fn only_running_foreign_apps_count() {
        let e: Vec<Usage> =
            vec![
            ("MSTeams_8wekyb3d8bbwe".into(), 10, 0),
            ("C:#Program Files#Google#Chrome#Application#chrome.exe".into(), 10, 20),
            (
                "C:#Users#Koava#AppData#Local#Hermes Assistant#hermes-assistant.exe".into(),
                10,
                0,
            ),
            (
                r"C:#Users#Koava#AppData#Local#hermes#tools#ffmpeg-9.0.1-win32-x64#bin#ffmpeg.exe"
                    .into(),
                10,
                0,
            ),
            ("MSTeams_8wekyb3d8bbwe".into(), 10, 0),
            ("C:#never#used.exe".into(), 0, 0),
        ];
        let s = active(&e);
        assert!(s.in_call);
        assert_eq!(s.apps, vec!["Teams"]);
        assert_eq!(active(&e[1..4]), CallStatus::default());
    }
}
