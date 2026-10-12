//! Program entry for Windows: wires wlanapi, IP Helper, the event log and the
//! console into the shared `Monitor`.

use std::path::PathBuf;
use std::sync::mpsc::{self, RecvTimeoutError};
use std::sync::Arc;
use std::time::Duration;

use super::console::{self, StdoutSink};
use super::evtlog::Subscriptions;
use super::iphlp;
use super::sysinfo;
use super::wlan::{InterfaceInfo, LinkReader, WlanClient};
use crate::cli;
use crate::events::MonitorInput;
use crate::monitor::{Monitor, MonitorConfig, Reading, SampleReason};
use crate::oui::Oui;
use crate::session_log::SessionLog;
use crate::Date;

/// Upper bound on one wait so the Ctrl handler flag is noticed promptly.
const MAX_WAIT: Duration = Duration::from_millis(200);

pub fn main() -> i32 {
    let arguments: Vec<String> = std::env::args().skip(1).collect();
    let options = match cli::parse(&arguments) {
        Ok(o) => o,
        Err(message) => {
            eprint!("error: {message}\n{}", cli::help_text());
            return 2;
        }
    };
    if options.help {
        print!("{}", cli::help_text());
        return 0;
    }

    let mut client = match WlanClient::open() {
        Ok(c) => c,
        Err(code) => {
            eprintln!("error: WlanOpenHandle failed (error {code}). Is the WLAN AutoConfig service running?");
            return 1;
        }
    };
    let interfaces = client.interfaces().unwrap_or_default();
    let Some(interface) = select_interface(&interfaces, options.interface_name.as_deref()) else {
        match &options.interface_name {
            Some(name) => eprintln!("error: no Wi-Fi interface matches {name:?}"),
            None => eprintln!("error: no Wi-Fi interface found"),
        }
        return 1;
    };

    let now = chrono::Local::now();
    let (session_log, session_log_warning) = open_session_log(&options, now, &arguments);
    let (oui, oui_warning) = Oui::find(&sysinfo::oui_candidates(options.oui_path.as_deref()));

    let config = MonitorConfig {
        options: options.clone(),
        interface_name: interface.description.clone(),
        os_line: sysinfo::os_line(),
        oui,
        oui_warning,
        session_log,
        session_log_warning,
    };
    let mut monitor = Monitor::new(config, StdoutSink::new(), now);
    monitor.start(now);

    console::install_ctrl_handler();
    let (sender, receiver) = mpsc::channel::<MonitorInput>();
    if let Err(code) = client.register_notifications(sender.clone()) {
        monitor.warn(
            &format!("WlanRegisterNotification failed (error {code}). JOIN TIMING and roam markers are off."),
            chrono::Local::now(),
        );
    }
    let (subscriptions, warnings) = Subscriptions::start(sender.clone());
    for warning in warnings {
        monitor.warn(&warning, chrono::Local::now());
    }
    drop(sender);

    let mut reader = LinkReader::new(interface.guid);
    let interval = Duration::from_secs_f64(options.interval.max(0.05));
    let mut now = chrono::Local::now();
    monitor.sample(
        read(&client, &mut reader, &interface, now),
        SampleReason::Startup,
        now,
    );
    let mut next_poll =
        now + chrono::Duration::from_std(interval).unwrap_or(chrono::Duration::seconds(1));

    while !console::stop_requested() {
        now = chrono::Local::now();
        let mut deadline = next_poll;
        if let Some(d) = monitor.next_deadline() {
            deadline = deadline.min(d);
        }
        let wait = (deadline - now)
            .to_std()
            .unwrap_or(Duration::ZERO)
            .min(MAX_WAIT);
        match receiver.recv_timeout(wait) {
            Ok(MonitorInput::Wlan(event, at)) => monitor.on_wlan_event(event, at),
            Ok(MonitorInput::Log(event, at)) => monitor.on_log_event(event, at),
            Ok(MonitorInput::Tick(_)) => {}
            Ok(MonitorInput::Stop) | Err(RecvTimeoutError::Disconnected) => break,
            Err(RecvTimeoutError::Timeout) => {}
        }
        now = chrono::Local::now();
        if now >= next_poll {
            monitor.sample(
                read(&client, &mut reader, &interface, now),
                SampleReason::Poll,
                now,
            );
            while next_poll <= now {
                next_poll +=
                    chrono::Duration::from_std(interval).unwrap_or(chrono::Duration::seconds(1));
            }
        } else if monitor.next_deadline().is_some_and(|d| d <= now) {
            monitor.sample(
                read(&client, &mut reader, &interface, now),
                SampleReason::Settle,
                now,
            );
        }
    }

    drop(subscriptions);
    drop(client);
    monitor.shutdown(chrono::Local::now());
    0
}

/// `-i` matches the interface GUID or a case-insensitive substring of the
/// adapter description. Without `-i` the first WLAN interface is used.
fn select_interface(interfaces: &[InterfaceInfo], wanted: Option<&str>) -> Option<InterfaceInfo> {
    let Some(wanted) = wanted else {
        return interfaces.first().cloned();
    };
    let needle = wanted
        .trim()
        .trim_matches(|c| c == '{' || c == '}')
        .to_ascii_lowercase();
    interfaces
        .iter()
        .find(|i| {
            i.guid_string
                .trim_matches(|c| c == '{' || c == '}')
                .eq_ignore_ascii_case(&needle)
        })
        .or_else(|| {
            interfaces
                .iter()
                .find(|i| i.description.to_ascii_lowercase().contains(&needle))
        })
        .cloned()
}

fn open_session_log(
    options: &cli::RunOptions,
    now: Date,
    arguments: &[String],
) -> (Option<Arc<SessionLog>>, Option<String>) {
    if !options.session_log {
        return (None, None);
    }
    let directory = match &options.log_directory {
        Some(dir) => PathBuf::from(dir),
        None => match sysinfo::default_log_directory() {
            Some(dir) => dir,
            None => return (None, Some("LOCALAPPDATA is not set".to_string())),
        },
    };
    match SessionLog::open(&directory, now, arguments) {
        Ok(log) => (Some(Arc::new(log)), None),
        Err(reason) => (None, Some(format!("{}: {reason}", directory.display()))),
    }
}

fn read(
    client: &WlanClient,
    reader: &mut LinkReader,
    interface: &InterfaceInfo,
    now: Date,
) -> Reading {
    let link = reader.read(client, now);
    let adapter = iphlp::adapter(&interface.guid_string);
    let counters = adapter.as_ref().and_then(|a| iphlp::counters(a.luid));
    Reading {
        link: link.state,
        ip: adapter.map(|a| a.ip).unwrap_or_default(),
        counters,
        bss_denied: link.bss_denied,
    }
}
