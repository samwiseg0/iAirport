//! OS description and well-known paths.

use std::ffi::c_void;
use std::path::PathBuf;
use std::ptr;

use windows_sys::Win32::Foundation::ERROR_SUCCESS;
use windows_sys::Win32::System::Registry::{RegGetValueW, HKEY_LOCAL_MACHINE, RRF_RT_REG_SZ};

use super::util::{from_wide, to_wide};

const VERSION_KEY: &str = r"SOFTWARE\Microsoft\Windows NT\CurrentVersion";

fn registry_string(value: &str) -> Option<String> {
    let key = to_wide(VERSION_KEY);
    let name = to_wide(value);
    let mut buffer = [0u16; 256];
    let mut size = (buffer.len() * 2) as u32;
    // SAFETY: size is the buffer length in bytes; the API writes within it.
    let code = unsafe {
        RegGetValueW(
            HKEY_LOCAL_MACHINE,
            key.as_ptr(),
            name.as_ptr(),
            RRF_RT_REG_SZ,
            ptr::null_mut(),
            buffer.as_mut_ptr() as *mut c_void,
            &mut size,
        )
    };
    (code == ERROR_SUCCESS)
        .then(|| from_wide(&buffer))
        .filter(|s| !s.is_empty())
}

/// Builds the header line's OS description, for example
/// `Windows 11 24H2 (build 26100)`. The registry ProductName still says
/// "Windows 10" on Windows 11, so the build number decides the major name.
pub fn os_line() -> String {
    let build = registry_string("CurrentBuild").and_then(|b| b.parse::<u32>().ok());
    let display = registry_string("DisplayVersion").or_else(|| registry_string("ReleaseId"));
    let product = match build {
        Some(b) if b >= 22000 => "Windows 11".to_string(),
        Some(_) => "Windows 10".to_string(),
        None => registry_string("ProductName").unwrap_or_else(|| "Windows".to_string()),
    };
    let mut line = product;
    if let Some(d) = display {
        line.push(' ');
        line.push_str(&d);
    }
    if let Some(b) = build {
        line.push_str(&format!(" (build {b})"));
    }
    line
}

pub fn local_app_data() -> Option<PathBuf> {
    std::env::var_os("LOCALAPPDATA").map(PathBuf::from)
}

pub fn program_data() -> Option<PathBuf> {
    std::env::var_os("ProgramData").map(PathBuf::from)
}

pub fn default_log_directory() -> Option<PathBuf> {
    local_app_data().map(|p| p.join("iairport").join("logs"))
}

/// OUI file search order: `--oui`, next to the exe, `%LOCALAPPDATA%`,
/// `%ProgramData%`, then the repo root in debug builds.
pub fn oui_candidates(explicit: Option<&str>) -> Vec<PathBuf> {
    let mut out = Vec::new();
    if let Some(path) = explicit {
        out.push(PathBuf::from(path));
    }
    if let Some(dir) = std::env::current_exe()
        .ok()
        .and_then(|p| p.parent().map(|d| d.to_path_buf()))
    {
        out.push(dir.join("oui.txt"));
    }
    if let Some(base) = local_app_data() {
        out.push(base.join("iairport").join("oui.txt"));
    }
    if let Some(base) = program_data() {
        out.push(base.join("iairport").join("oui.txt"));
    }
    if cfg!(debug_assertions) {
        out.push(
            PathBuf::from(env!("CARGO_MANIFEST_DIR"))
                .join("..")
                .join("oui.txt"),
        );
    }
    out
}
