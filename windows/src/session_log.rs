use std::fs::{self, File, OpenOptions};
use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::Mutex;

use crate::format::time;
use crate::Date;

pub struct SessionLog {
    pub path: PathBuf,
    file: Mutex<Option<File>>,
}

pub fn file_name(now: Date, pid: Option<u32>) -> String {
    let stamp = now.format("%Y%m%d-%H%M%S");
    match pid {
        Some(pid) => format!("iairport-{stamp}-{pid}.log"),
        None => format!("iairport-{stamp}.log"),
    }
}

pub fn first_line(now: Date, pid: u32, arguments: &[String]) -> String {
    let args = if arguments.is_empty() {
        "(none)".to_string()
    } else {
        arguments.join(" ")
    };
    format!(
        "iairport session log  started {}  pid {}  args: {}",
        time::json(&now),
        pid,
        args
    )
}

impl SessionLog {
    pub fn open(directory: &Path, now: Date, arguments: &[String]) -> Result<SessionLog, String> {
        fs::create_dir_all(directory).map_err(|e| e.to_string())?;
        let pid = std::process::id();
        let mut path = directory.join(file_name(now, None));
        let file = match create_new(&path) {
            Ok(f) => f,
            Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => {
                path = directory.join(file_name(now, Some(pid)));
                create_new(&path).map_err(|e| e.to_string())?
            }
            Err(e) => return Err(e.to_string()),
        };
        let log = SessionLog {
            path: path.clone(),
            file: Mutex::new(Some(file)),
        };
        if let Err(e) = log.write_raw(&format!("{}\n", first_line(now, pid, arguments))) {
            log.close();
            let _ = fs::remove_file(&path);
            return Err(e.to_string());
        }
        Ok(log)
    }

    pub fn write(&self, line: &str) {
        let _ = self.write_raw(&format!("{line}\n"));
    }

    fn write_raw(&self, text: &str) -> std::io::Result<()> {
        let mut guard = self.file.lock().unwrap_or_else(|p| p.into_inner());
        if let Some(file) = guard.as_mut() {
            file.write_all(text.as_bytes())?;
        }
        Ok(())
    }

    pub fn close(&self) {
        let mut guard = self.file.lock().unwrap_or_else(|p| p.into_inner());
        if let Some(mut file) = guard.take() {
            let _ = file.flush();
        }
    }
}

impl Drop for SessionLog {
    fn drop(&mut self) {
        self.close();
    }
}

fn create_new(path: &Path) -> std::io::Result<File> {
    OpenOptions::new().append(true).create_new(true).open(path)
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::{Local, TimeZone};

    #[test]
    fn names_and_first_line() {
        let now = Local.with_ymd_and_hms(2026, 3, 4, 10, 11, 12).unwrap();
        assert_eq!(file_name(now, None), "iairport-20260304-101112.log");
        assert_eq!(file_name(now, Some(42)), "iairport-20260304-101112-42.log");
        let line = first_line(now, 7, &[]);
        assert!(line.starts_with("iairport session log  started 2026-03-04T10:11:12.000"));
        assert!(line.ends_with("  pid 7  args: (none)"));
        assert!(first_line(now, 7, &["-v".into(), "-l".into()]).ends_with("args: -v -l"));
    }

    #[test]
    fn collision_adds_pid() {
        let dir = std::env::current_dir()
            .unwrap()
            .join("target")
            .join("session-log-test");
        let _ = fs::remove_dir_all(&dir);
        let now = Local::now();
        let a = SessionLog::open(&dir, now, &[]).unwrap();
        let b = SessionLog::open(&dir, now, &["-v".into()]).unwrap();
        assert_ne!(a.path, b.path);
        assert!(b
            .path
            .to_string_lossy()
            .contains(&format!("-{}.log", std::process::id())));
        a.write("hello");
        drop(a);
        let text = fs::read_to_string(dir.join(file_name(now, None))).unwrap();
        assert!(text.ends_with("hello\n"));
        assert!(text.starts_with("iairport session log  started "));
        drop(b);
        let _ = fs::remove_dir_all(&dir);
    }
}
