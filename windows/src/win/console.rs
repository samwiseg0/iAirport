//! Console output sink and Ctrl handler.

use std::io::Write;
use std::sync::atomic::{AtomicBool, Ordering};

use windows_sys::core::BOOL;
use windows_sys::Win32::Foundation::HANDLE;
use windows_sys::Win32::System::Console::{
    GetConsoleMode, GetConsoleScreenBufferInfo, GetStdHandle, SetConsoleCtrlHandler,
    SetConsoleMode, CONSOLE_SCREEN_BUFFER_INFO, CTRL_BREAK_EVENT, CTRL_CLOSE_EVENT, CTRL_C_EVENT,
    ENABLE_VIRTUAL_TERMINAL_PROCESSING, STD_OUTPUT_HANDLE,
};

use crate::output::Sink;

static STOP: AtomicBool = AtomicBool::new(false);

pub fn stop_requested() -> bool {
    STOP.load(Ordering::SeqCst)
}

unsafe extern "system" fn ctrl_handler(ctrl_type: u32) -> BOOL {
    match ctrl_type {
        CTRL_C_EVENT | CTRL_BREAK_EVENT | CTRL_CLOSE_EVENT => {
            STOP.store(true, Ordering::SeqCst);
            // On CTRL_CLOSE_EVENT Windows kills the process once this
            // returns, so the main loop gets a short window to print the
            // summary. Sleeping here keeps the process alive for that window.
            if ctrl_type == CTRL_CLOSE_EVENT {
                std::thread::sleep(std::time::Duration::from_millis(1500));
            }
            1
        }
        _ => 0,
    }
}

pub fn install_ctrl_handler() {
    // SAFETY: the handler is a plain `extern "system"` function that only
    // touches an atomic.
    unsafe {
        SetConsoleCtrlHandler(Some(ctrl_handler), 1);
    }
}

pub struct StdoutSink {
    handle: HANDLE,
    is_tty: bool,
    stdout: std::io::Stdout,
}

impl StdoutSink {
    pub fn new() -> Self {
        // SAFETY: GetStdHandle has no preconditions.
        let handle = unsafe { GetStdHandle(STD_OUTPUT_HANDLE) };
        let mut mode = 0u32;
        // SAFETY: handle is the process stdout; mode is a valid out-pointer.
        // GetConsoleMode fails when stdout is a pipe or file.
        let is_tty = unsafe { GetConsoleMode(handle, &mut mode) } != 0;
        if is_tty {
            // SAFETY: same handle; a failure here only means no colour.
            unsafe {
                SetConsoleMode(handle, mode | ENABLE_VIRTUAL_TERMINAL_PROCESSING);
            }
        }
        StdoutSink {
            handle,
            is_tty,
            stdout: std::io::stdout(),
        }
    }
}

impl Default for StdoutSink {
    fn default() -> Self {
        Self::new()
    }
}

impl Sink for StdoutSink {
    fn write_raw(&mut self, text: &str) {
        let mut lock = self.stdout.lock();
        let _ = lock.write_all(text.as_bytes());
        let _ = lock.flush();
    }

    fn is_tty(&self) -> bool {
        self.is_tty
    }

    fn columns(&self) -> Option<usize> {
        if !self.is_tty {
            return None;
        }
        let mut info = CONSOLE_SCREEN_BUFFER_INFO::default();
        // SAFETY: handle is the console; info is a valid out-pointer.
        let ok = unsafe { GetConsoleScreenBufferInfo(self.handle, &mut info) };
        if ok == 0 {
            return None;
        }
        let width = i32::from(info.srWindow.Right) - i32::from(info.srWindow.Left) + 1;
        usize::try_from(width).ok().filter(|w| *w > 0)
    }
}
