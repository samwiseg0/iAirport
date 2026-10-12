#[derive(Clone, Debug, PartialEq)]
pub struct RunOptions {
    pub verbose: bool,
    pub log: bool,
    pub interface_name: Option<String>,
    pub oui_path: Option<String>,
    pub no_color: bool,
    pub json: bool,
    pub interval: f64,
    pub help: bool,
    pub session_log: bool,
    pub log_directory: Option<String>,
}

impl Default for RunOptions {
    fn default() -> Self {
        RunOptions {
            verbose: false,
            log: false,
            interface_name: None,
            oui_path: None,
            no_color: false,
            json: false,
            interval: 1.0,
            help: false,
            session_log: true,
            log_directory: None,
        }
    }
}

pub fn parse(arguments: &[String]) -> Result<RunOptions, String> {
    let mut options = RunOptions::default();
    let mut index = 0;
    while index < arguments.len() {
        let arg = arguments[index].as_str();
        match arg {
            "-v" | "--verbose" => options.verbose = true,
            "-l" | "--log" => options.log = true,
            "-h" | "--help" => options.help = true,
            "--no-color" => options.no_color = true,
            "--no-session-log" => options.session_log = false,
            "--log-dir" => {
                index += 1;
                let Some(value) = arguments.get(index) else {
                    return Err("missing value for --log-dir".to_string());
                };
                options.log_directory = Some(value.clone());
            }
            "--json" => {
                options.json = true;
                options.no_color = true;
            }
            "-i" | "--interface" => {
                index += 1;
                let Some(value) = arguments.get(index) else {
                    return Err(format!("missing value for {arg}"));
                };
                options.interface_name = Some(value.clone());
            }
            "--oui" => {
                index += 1;
                let Some(value) = arguments.get(index) else {
                    return Err("missing value for --oui".to_string());
                };
                options.oui_path = Some(value.clone());
            }
            "--interval" => {
                index += 1;
                let value = arguments.get(index).and_then(|v| v.parse::<f64>().ok());
                match value {
                    Some(v) if v.is_finite() && v > 0.0 && v <= 86_400.0 => options.interval = v,
                    _ => return Err("--interval needs a finite number from 0 to 86400".to_string()),
                }
            }
            _ if arg.starts_with('-') => return Err(format!("unknown option {arg}")),
            _ => return Err(format!("unexpected argument {arg}")),
        }
        index += 1;
    }
    Ok(options)
}

pub fn help_text() -> String {
    "iairport [options]
  -v, --verbose         print extra WLAN event-log lines and IP tags
  -l, --log             write iairport-samples.csv, iairport-roams.csv and bssid_list.txt
  -i, --interface X     Wi-Fi interface GUID or description. Default is the first WLAN interface
      --oui PATH        path to oui.txt
      --no-color        plain output
      --no-session-log  do not write the session log under %LOCALAPPDATA%\\iairport\\logs
      --log-dir PATH    folder for the session log. Default is %LOCALAPPDATA%\\iairport\\logs
      --json            write newline-delimited JSON
      --interval N      sample interval in seconds. Default is 1
  -h, --help            show help"
        .to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn args(list: &[&str]) -> Vec<String> {
        list.iter().map(|s| s.to_string()).collect()
    }

    #[test]
    fn parses_all_flags() {
        let o = parse(&args(&[
            "-v",
            "-l",
            "-i",
            "Wi-Fi",
            "--oui",
            "x.txt",
            "--no-color",
            "--no-session-log",
            "--log-dir",
            "C:\\logs",
            "--interval",
            "0.5",
        ]))
        .unwrap();
        assert!(o.verbose && o.log && o.no_color && !o.session_log);
        assert_eq!(o.interface_name.as_deref(), Some("Wi-Fi"));
        assert_eq!(o.oui_path.as_deref(), Some("x.txt"));
        assert_eq!(o.log_directory.as_deref(), Some("C:\\logs"));
        assert_eq!(o.interval, 0.5);
        let j = parse(&args(&["--json"])).unwrap();
        assert!(j.json && j.no_color);
        assert!(parse(&args(&["-h"])).unwrap().help);
    }

    #[test]
    fn interval_validation() {
        let msg = "--interval needs a finite number from 0 to 86400";
        assert_eq!(parse(&args(&["--interval", "0"])).unwrap_err(), msg);
        assert_eq!(parse(&args(&["--interval", "-1"])).unwrap_err(), msg);
        assert_eq!(parse(&args(&["--interval", "86401"])).unwrap_err(), msg);
        assert_eq!(parse(&args(&["--interval", "inf"])).unwrap_err(), msg);
        assert_eq!(parse(&args(&["--interval", "nan"])).unwrap_err(), msg);
        assert_eq!(parse(&args(&["--interval", "abc"])).unwrap_err(), msg);
        assert_eq!(parse(&args(&["--interval"])).unwrap_err(), msg);
        assert_eq!(
            parse(&args(&["--interval", "86400"])).unwrap().interval,
            86400.0
        );
    }

    #[test]
    fn errors() {
        assert_eq!(parse(&args(&["-i"])).unwrap_err(), "missing value for -i");
        assert_eq!(
            parse(&args(&["--oui"])).unwrap_err(),
            "missing value for --oui"
        );
        assert_eq!(
            parse(&args(&["--log-dir"])).unwrap_err(),
            "missing value for --log-dir"
        );
        assert_eq!(parse(&args(&["-d"])).unwrap_err(), "unknown option -d");
        assert_eq!(
            parse(&args(&["--sudo-log"])).unwrap_err(),
            "unknown option --sudo-log"
        );
        assert_eq!(
            parse(&args(&["--no-notify"])).unwrap_err(),
            "unknown option --no-notify"
        );
        assert_eq!(
            parse(&args(&["foo"])).unwrap_err(),
            "unexpected argument foo"
        );
    }

    #[test]
    fn help_lists_windows_flags_only() {
        let help = help_text();
        assert!(help.starts_with("iairport [options]\n"));
        assert!(help.contains("--interval N"));
        assert!(!help.contains("--sudo-log"));
        assert!(!help.contains("--debug"));
        assert!(!help.contains("--no-notify"));
    }
}
