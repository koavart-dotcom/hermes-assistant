//! Fast local speech recognition for always-listening mode: Vosk (small Russian model,
//! Kaldi) hears the wake word and quick commands while Artem is still speaking — words
//! show up in partial results ~0.2 s after they are said, where Whisper only starts
//! after the phrase has ended. Whisper stays for everything that goes to the model.
//!
//! The window streams 16 kHz PCM16 to [`feed`]; the engine is `libvosk.dll` (official
//! vosk-api 0.3.45 build, MinGW) shipped inside the installer (`resources/vosk`, put
//! there by the release workflow) and loaded with `libloading`, so nothing is linked at
//! build time. The app never downloads executable code: an exe that fetches and loads a
//! DLL looked like a dropper to Defender's ML (`Trojan:Win32/Bearfoos.B!ml`, 17.10.2026).
//! Only the model (data) is fetched once into `<app data>/vosk`, checked by SHA-256.

use std::path::{Path, PathBuf};

use futures_util::StreamExt;
use serde::Serialize;
use tokio::io::AsyncWriteExt;
use ts_rs::TS;

use crate::{CoreError, Layout, Result};

pub const MODEL: &str = "vosk-model-small-ru-0.22";
const MODEL_URL: &str = "https://alphacephei.com/vosk/models/vosk-model-small-ru-0.22.zip";
const MODEL_SHA256: &str = "961d5ff98a17f4aa6de69864d0aa71fa5bac682301d2b5d17a3f24c5c99a46d4";
pub const SAMPLE_RATE: f32 = 16_000.0;
#[cfg(windows)]
const LIB_FILE: &str = "libvosk.dll";
#[cfg(not(windows))]
const LIB_FILE: &str = "libvosk.so";

/// What the recogniser heard after a chunk of audio.
#[derive(Debug, Clone, Default, Serialize, TS, PartialEq)]
#[ts(export, export_to = "../../src/types/generated/")]
pub struct VoskHeard {
    /// The phrase so far (changes while Artem speaks); empty after a final result.
    pub partial: String,
    /// Set once when Vosk decides the phrase has ended.
    pub text: Option<String>,
}

pub fn dir(layout: &Layout) -> PathBuf {
    layout.app_data_dir.join("vosk")
}

pub fn model_dir(layout: &Layout) -> PathBuf {
    dir(layout).join(MODEL)
}

fn sha256_hex(path: &Path) -> Result<String> {
    use sha2::Digest;
    let mut hasher = sha2::Sha256::new();
    let mut f = std::fs::File::open(path)?;
    std::io::copy(&mut f, &mut hasher)?;
    Ok(hex::encode(hasher.finalize()))
}

async fn fetch(url: &str, dest: &Path, sha256: &str) -> Result<()> {
    log::info!("vosk: downloading {url}");
    let resp = reqwest::get(url).await?.error_for_status()?;
    let part = dest.with_extension("part");
    let mut file = tokio::fs::File::create(&part).await?;
    let mut stream = resp.bytes_stream();
    while let Some(chunk) = stream.next().await {
        file.write_all(&chunk?).await?;
    }
    file.flush().await?;
    drop(file);
    let p = part.clone();
    let got = tokio::task::spawn_blocking(move || sha256_hex(&p))
        .await
        .map_err(|e| CoreError::Other(e.to_string()))??;
    if got != sha256 {
        let _ = tokio::fs::remove_file(&part).await;
        return Err(CoreError::Other(format!(
            "{url}: checksum mismatch ({got})"
        )));
    }
    tokio::fs::rename(&part, dest).await?;
    Ok(())
}

/// Extract `zip` into `dest`; `strip_top` drops the archive's top folder.
pub(crate) fn unzip(zip_path: &Path, dest: &Path, strip_top: bool) -> Result<()> {
    let mut zip = zip::ZipArchive::new(std::fs::File::open(zip_path)?)?;
    for i in 0..zip.len() {
        let mut entry = zip.by_index(i)?;
        let Some(rel) = entry.enclosed_name() else {
            continue;
        };
        let rel: PathBuf = if strip_top {
            rel.components().skip(1).collect()
        } else {
            rel
        };
        if rel.as_os_str().is_empty() {
            continue;
        }
        let out = dest.join(rel);
        if entry.is_dir() {
            std::fs::create_dir_all(&out)?;
            continue;
        }
        if let Some(parent) = out.parent() {
            std::fs::create_dir_all(parent)?;
        }
        std::io::copy(&mut entry, &mut std::fs::File::create(&out)?)?;
    }
    Ok(())
}

/// Fetch the model once (≈45 MB); a marker file means «complete».
pub async fn ensure(layout: &Layout) -> Result<()> {
    let root = dir(layout);
    let marker = root.join("model.ok");
    if marker.is_file() {
        return Ok(());
    }
    tokio::fs::create_dir_all(&root).await?;
    let zip_path = root.join("download.zip");
    fetch(MODEL_URL, &zip_path, MODEL_SHA256).await?;
    let (z, t) = (zip_path.clone(), root.clone());
    tokio::task::spawn_blocking(move || unzip(&z, &t, false))
        .await
        .map_err(|e| CoreError::Other(e.to_string()))??;
    let _ = tokio::fs::remove_file(&zip_path).await;
    tokio::fs::write(&marker, MODEL_SHA256).await?;
    Ok(())
}

/// Vosk JSON (`{"partial": …}` / `{"text": …}`) → the field's text.
fn field(json: &str, key: &str) -> String {
    serde_json::from_str::<serde_json::Value>(json)
        .ok()
        .and_then(|v| v.get(key).and_then(|t| t.as_str()).map(str::to_string))
        .unwrap_or_default()
}

mod engine {
    use std::ffi::{CStr, CString, c_char, c_float, c_int, c_short, c_void};
    use std::path::Path;

    use libloading::{Library, Symbol};

    use crate::{CoreError, Result};

    type ModelNew = unsafe extern "C" fn(*const c_char) -> *mut c_void;
    type RecNew = unsafe extern "C" fn(*mut c_void, c_float) -> *mut c_void;
    type Accept = unsafe extern "C" fn(*mut c_void, *const c_short, c_int) -> c_int;
    type GetText = unsafe extern "C" fn(*mut c_void) -> *const c_char;
    type Handle = unsafe extern "C" fn(*mut c_void);
    type SetLog = unsafe extern "C" fn(c_int);

    pub struct Engine {
        model: *mut c_void,
        rec: *mut c_void,
        accept: Accept,
        partial: GetText,
        result: GetText,
        reset: Handle,
        free_rec: Handle,
        free_model: Handle,
        // Last: the function pointers above live as long as the library.
        _lib: Library,
    }

    // SAFETY: the handles are only used behind the module's Mutex, one thread at a time.
    unsafe impl Send for Engine {}

    fn sym<T: Copy>(lib: &Library, name: &[u8]) -> Result<T> {
        // SAFETY: names and signatures follow vosk_api.h of the pinned release.
        unsafe { lib.get::<T>(name) }
            .map(|s: Symbol<T>| *s)
            .map_err(|e| CoreError::Other(format!("libvosk: {e}")))
    }

    #[cfg(windows)]
    fn open(lib_dir: &Path) -> Result<Library> {
        use libloading::os::windows::{LOAD_WITH_ALTERED_SEARCH_PATH, Library as WinLibrary};
        // Altered search path: libvosk.dll finds libstdc++/libgcc/winpthread next to it.
        // SAFETY: loading the pinned official build.
        unsafe {
            WinLibrary::load_with_flags(
                lib_dir.join(super::LIB_FILE),
                LOAD_WITH_ALTERED_SEARCH_PATH,
            )
        }
        .map(Library::from)
        .map_err(|e| CoreError::Other(format!("libvosk.dll: {e}")))
    }

    #[cfg(not(windows))]
    fn open(lib_dir: &Path) -> Result<Library> {
        // SAFETY: loading libvosk (development and tests only).
        unsafe { Library::new(lib_dir.join(super::LIB_FILE)) }
            .map_err(|e| CoreError::Other(format!("libvosk.so: {e}")))
    }

    impl Engine {
        pub fn load(lib_dir: &Path, model_dir: &Path, sample_rate: f32) -> Result<Engine> {
            let lib = open(lib_dir)?;
            let set_log: SetLog = sym(&lib, b"vosk_set_log_level\0")?;
            let model_new: ModelNew = sym(&lib, b"vosk_model_new\0")?;
            let rec_new: RecNew = sym(&lib, b"vosk_recognizer_new\0")?;
            let free_model: Handle = sym(&lib, b"vosk_model_free\0")?;
            let path = CString::new(model_dir.to_string_lossy().as_bytes())
                .map_err(|e| CoreError::Other(e.to_string()))?;
            // SAFETY: valid C string; null result is checked.
            let model = unsafe {
                set_log(-1);
                model_new(path.as_ptr())
            };
            if model.is_null() {
                return Err(CoreError::Other(format!(
                    "vosk: cannot load model {}",
                    model_dir.display()
                )));
            }
            // SAFETY: model is a live handle.
            let rec = unsafe { rec_new(model, sample_rate) };
            if rec.is_null() {
                // SAFETY: created above, not used afterwards.
                unsafe { free_model(model) };
                return Err(CoreError::Other("vosk: cannot create recogniser".into()));
            }
            Ok(Engine {
                accept: sym(&lib, b"vosk_recognizer_accept_waveform_s\0")?,
                partial: sym(&lib, b"vosk_recognizer_partial_result\0")?,
                result: sym(&lib, b"vosk_recognizer_result\0")?,
                reset: sym(&lib, b"vosk_recognizer_reset\0")?,
                free_rec: sym(&lib, b"vosk_recognizer_free\0")?,
                free_model,
                model,
                rec,
                _lib: lib,
            })
        }

        fn text(&self, f: GetText) -> String {
            // SAFETY: Vosk returns a NUL-terminated string owned by the recogniser.
            unsafe {
                let p = f(self.rec);
                if p.is_null() {
                    String::new()
                } else {
                    CStr::from_ptr(p).to_string_lossy().into_owned()
                }
            }
        }

        /// (phrase ended, JSON of the partial or the final result).
        pub fn accept(&mut self, pcm: &[i16]) -> (bool, String) {
            // SAFETY: pointer and length of a live slice.
            let ended = unsafe { (self.accept)(self.rec, pcm.as_ptr(), pcm.len() as c_int) } == 1;
            let json = if ended {
                self.text(self.result)
            } else {
                self.text(self.partial)
            };
            (ended, json)
        }

        pub fn reset(&mut self) {
            // SAFETY: live handle.
            unsafe { (self.reset)(self.rec) }
        }
    }

    impl Drop for Engine {
        fn drop(&mut self) {
            // SAFETY: handles created in `load`, freed once.
            unsafe {
                (self.free_rec)(self.rec);
                (self.free_model)(self.model);
            }
        }
    }
}

static ENGINE: std::sync::Mutex<Option<engine::Engine>> = std::sync::Mutex::new(None);

fn lock() -> std::sync::MutexGuard<'static, Option<engine::Engine>> {
    ENGINE.lock().unwrap_or_else(|e| e.into_inner())
}

/// Download if needed and load the engine (idempotent). Blocking work runs off the async pool.
/// `lib_dir` — the bundled `resources/vosk` with `libvosk.dll` and its MinGW runtime.
pub async fn start(layout: &Layout, lib_dir: &Path) -> Result<()> {
    if lock().is_some() {
        return Ok(());
    }
    if !lib_dir.join(LIB_FILE).is_file() {
        return Err(CoreError::Other(format!(
            "{LIB_FILE} is not bundled in {}",
            lib_dir.display()
        )));
    }
    ensure(layout).await?;
    let (lib, model) = (lib_dir.to_path_buf(), model_dir(layout));
    let engine =
        tokio::task::spawn_blocking(move || engine::Engine::load(&lib, &model, SAMPLE_RATE))
            .await
            .map_err(|e| CoreError::Other(e.to_string()))??;
    *lock() = Some(engine);
    log::info!("vosk: engine ready ({MODEL})");
    Ok(())
}

/// Feed little-endian PCM16 at 16 kHz.
pub fn feed(bytes: &[u8]) -> Result<VoskHeard> {
    let pcm: Vec<i16> = bytes
        .chunks_exact(2)
        .map(|b| i16::from_le_bytes([b[0], b[1]]))
        .collect();
    let mut guard = lock();
    let engine = guard
        .as_mut()
        .ok_or_else(|| CoreError::Other("vosk is not started".into()))?;
    let (ended, json) = engine.accept(&pcm);
    Ok(if ended {
        VoskHeard {
            partial: String::new(),
            text: Some(field(&json, "text")),
        }
    } else {
        VoskHeard {
            partial: field(&json, "partial"),
            text: None,
        }
    })
}

/// Forget the current phrase (after we acted on it, or after our own voice).
pub fn reset() {
    if let Some(e) = lock().as_mut() {
        e.reset();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;

    #[test]
    fn reads_vosk_json() {
        assert_eq!(
            field(r#"{"partial" : "джарвис пауза"}"#, "partial"),
            "джарвис пауза"
        );
        assert_eq!(field(r#"{"text" : ""}"#, "text"), "");
        assert_eq!(field("garbage", "text"), "");
    }

    #[test]
    fn unzips_with_and_without_top_folder() {
        let root = std::env::temp_dir().join(format!("hermes-vosk-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(&root).unwrap();
        let zip_path = root.join("v.zip");
        {
            let mut w = zip::ZipWriter::new(std::fs::File::create(&zip_path).unwrap());
            let opts = zip::write::SimpleFileOptions::default();
            w.add_directory("vosk-win64/", opts).unwrap();
            w.start_file("vosk-win64/libvosk.dll", opts).unwrap();
            w.write_all(b"dll").unwrap();
            w.finish().unwrap();
        }
        unzip(&zip_path, &root.join("bin"), true).unwrap();
        assert_eq!(std::fs::read(root.join("bin/libvosk.dll")).unwrap(), b"dll");
        unzip(&zip_path, &root.join("keep"), false).unwrap();
        assert!(root.join("keep/vosk-win64/libvosk.dll").is_file());
        assert_eq!(sha256_hex(&root.join("bin/libvosk.dll")).unwrap().len(), 64);
        let _ = std::fs::remove_dir_all(&root);
    }

    /// Real engine on a recording: `VOSK_LIB_DIR=<dir with libvosk.so> VOSK_MODEL=<model dir>
    /// VOSK_WAV=<16 kHz mono wav> cargo test -p hermes-core vosk_live -- --ignored --nocapture`.
    #[test]
    #[ignore]
    fn vosk_live() {
        let var = |k: &str| PathBuf::from(std::env::var(k).unwrap());
        let mut e =
            engine::Engine::load(&var("VOSK_LIB_DIR"), &var("VOSK_MODEL"), SAMPLE_RATE).unwrap();
        let wav = std::fs::read(var("VOSK_WAV")).unwrap();
        let mut heard = Vec::new();
        for chunk in wav[44..].chunks(3200) {
            let pcm: Vec<i16> = chunk
                .chunks_exact(2)
                .map(|b| i16::from_le_bytes([b[0], b[1]]))
                .collect();
            let (ended, json) = e.accept(&pcm);
            if ended {
                heard.push(field(&json, "text"));
            }
        }
        println!("{heard:?}");
        assert!(heard.iter().any(|t| t.starts_with("джарвис")));
    }

    #[test]
    fn feed_without_engine_is_an_error() {
        assert!(feed(&[0, 0]).is_err());
    }
}
