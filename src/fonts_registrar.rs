//! Register the bundled IBM Plex Mono faces with the OS font system so the
//! subsequent `load_font("IBM Plex Mono", …)` calls (via `crossfont`) resolve
//! against the shipped copy instead of whatever the user has installed. All
//! registration is process-scoped: it does not touch the user's system font
//! table and disappears when the process exits.

use crate::fonts;

/// Register every bundled face. Idempotent within a process (repeat calls are
/// cheap no-ops on the fast path); safe to call from `main()` before any
/// renderer construction. Failures are logged and swallowed — koi continues
/// with whatever the OS already has, matching the pre-bundle behaviour.
pub fn register_bundled_fonts() {
    imp::register(&fonts::ALL);
}

#[cfg(target_os = "windows")]
mod imp {
    use std::io::Write;
    use std::os::windows::ffi::OsStrExt;
    use std::path::PathBuf;
    use std::sync::Once;
    use windows::core::PCWSTR;
    use windows::Win32::Graphics::Gdi::{AddFontResourceExW, FR_PRIVATE};

    static ONCE: Once = Once::new();

    // AddFontMemResourceEx is NOT enumerable by DirectWrite (per MSDN), so
    // crossfont's DirectWrite lookup can't resolve the font by name.
    // AddFontResourceExW + FR_PRIVATE is enumerable and process-scoped.
    pub fn register(faces: &[&[u8]]) {
        ONCE.call_once(|| {
            let Some(dir) = extract_dir() else {
                log::warn!("Bundled font extraction dir unavailable; skipping registration");
                return;
            };
            if let Err(e) = std::fs::create_dir_all(&dir) {
                log::warn!("Failed to create bundled-font dir {:?}: {}", dir, e);
                return;
            }
            for (i, bytes) in faces.iter().enumerate() {
                let path = dir.join(format!("koi-plexmono-{}.otf", i));
                if !path.exists() {
                    match std::fs::File::create(&path).and_then(|mut f| f.write_all(bytes)) {
                        Ok(()) => {}
                        Err(e) => {
                            log::warn!("Failed to write bundled font to {:?}: {}", path, e);
                            continue;
                        }
                    }
                }
                let wide: Vec<u16> = path.as_os_str().encode_wide().chain(std::iter::once(0)).collect();
                let added = unsafe {
                    AddFontResourceExW(PCWSTR(wide.as_ptr()), FR_PRIVATE, None)
                };
                if added == 0 {
                    log::warn!("AddFontResourceExW failed for {:?}", path);
                } else {
                    log::info!("Registered bundled font #{} via GDI/DirectWrite ({:?})", i, path);
                }
            }
        });
    }

    fn extract_dir() -> Option<PathBuf> {
        std::env::var_os("LOCALAPPDATA")
            .map(PathBuf::from)
            .or_else(|| std::env::var_os("TEMP").map(PathBuf::from))
            .map(|base| base.join("koi").join("fonts"))
    }
}

#[cfg(target_os = "macos")]
mod imp {
    use std::os::raw::c_void;
    use std::sync::Once;
    use core_graphics::{data_provider::CGDataProvider, font::CGFont};
    use foreign_types_shared::ForeignType;

    static ONCE: Once = Once::new();

    // `core-text` 22 no longer exports the graphics-font registration
    // extern, so we bind it ourselves. Both symbols are stable ABI in
    // CoreText.framework.
    #[link(name = "CoreText", kind = "framework")]
    extern "C" {
        fn CTFontManagerRegisterGraphicsFont(font: *const c_void, error: *mut *mut c_void) -> bool;
    }

    pub fn register(faces: &[&[u8]]) {
        ONCE.call_once(|| {
            for (i, bytes) in faces.iter().enumerate() {
                // SAFETY: `bytes` is `'static` (from `include_bytes!`), so the
                // slice lives for the process lifetime — longer than the
                // resulting CGDataProvider.
                let provider = unsafe { CGDataProvider::from_slice(bytes) };
                let font = match CGFont::from_data_provider(provider) {
                    Ok(f) => f,
                    Err(_) => {
                        log::warn!(
                            "CGFont::from_data_provider failed for bundled font #{}",
                            i
                        );
                        continue;
                    }
                };
                // `CTFontManagerRegisterGraphicsFont` is always process-scoped
                // and does not persist across restarts.
                let ok = unsafe {
                    CTFontManagerRegisterGraphicsFont(
                        font.as_ptr() as *const c_void,
                        std::ptr::null_mut(),
                    )
                };
                if !ok {
                    log::warn!(
                        "CTFontManagerRegisterGraphicsFont returned false for bundled font #{}",
                        i
                    );
                } else {
                    log::info!("Registered bundled font #{} via CoreText", i);
                }
            }
        });
    }
}

#[cfg(all(unix, not(target_os = "macos")))]
mod imp {
    use std::ffi::CString;
    use std::io::Write;
    use std::os::raw::{c_char, c_int, c_void};
    use std::path::PathBuf;
    use std::sync::Once;

    static ONCE: Once = Once::new();

    // fontconfig is loaded transitively via `crossfont` on Linux; we bind the
    // two functions we need directly so we don't add a new dependency.
    #[link(name = "fontconfig")]
    extern "C" {
        fn FcConfigGetCurrent() -> *mut c_void;
        fn FcConfigAppFontAddFile(config: *mut c_void, file: *const c_char) -> c_int;
    }

    pub fn register(faces: &[&[u8]]) {
        ONCE.call_once(|| {
            let Some(dir) = extract_dir() else {
                log::warn!("Bundled font extraction dir unavailable; skipping registration");
                return;
            };
            if let Err(e) = std::fs::create_dir_all(&dir) {
                log::warn!("Failed to create bundled-font dir {:?}: {}", dir, e);
                return;
            }

            let cfg = unsafe { FcConfigGetCurrent() };
            if cfg.is_null() {
                log::warn!("FcConfigGetCurrent returned null; skipping bundled font registration");
                return;
            }

            for (i, bytes) in faces.iter().enumerate() {
                let path = dir.join(format!("koi-plexmono-{}.otf", i));
                if !path.exists() {
                    match std::fs::File::create(&path).and_then(|mut f| f.write_all(bytes)) {
                        Ok(()) => {}
                        Err(e) => {
                            log::warn!("Failed to write bundled font to {:?}: {}", path, e);
                            continue;
                        }
                    }
                }
                let Ok(c_path) = CString::new(path.as_os_str().as_encoded_bytes()) else {
                    log::warn!("Bundled font path {:?} not representable as C string", path);
                    continue;
                };
                let ok = unsafe { FcConfigAppFontAddFile(cfg, c_path.as_ptr()) };
                if ok == 0 {
                    log::warn!("FcConfigAppFontAddFile failed for {:?}", path);
                } else {
                    log::info!("Registered bundled font #{} via fontconfig ({:?})", i, path);
                }
            }
        });
    }

    fn extract_dir() -> Option<PathBuf> {
        // Prefer $XDG_RUNTIME_DIR (per-user, tmpfs-backed, cleaned at logout);
        // fall back to std::env::temp_dir() so we still work on distros that
        // don't set XDG_RUNTIME_DIR.
        let base = std::env::var_os("XDG_RUNTIME_DIR")
            .map(PathBuf::from)
            .unwrap_or_else(std::env::temp_dir);
        Some(base.join("koi-fonts"))
    }
}

#[cfg(all(test, unix, not(target_os = "macos")))]
mod tests {
    use super::*;
    use crossfont::{FontDesc, Rasterize, Rasterizer, Size, Slant, Style, Weight};

    #[test]
    fn bundled_plex_mono_resolves_via_fontconfig() {
        register_bundled_fonts();

        let mut r = Rasterizer::new().expect("rasterizer");
        let desc = FontDesc::new(
            "IBM Plex Mono",
            Style::Description { slant: Slant::Normal, weight: Weight::Normal },
        );
        r.load_font(&desc, Size::new(14.0))
            .expect("bundled IBM Plex Mono should resolve after register_bundled_fonts()");
    }
}
