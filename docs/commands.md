# Flags

iairport uses simple flags. Unknown flags are usage errors. Use `iairport --help` to print the binary help.

## Options

| Flag | Default | Meaning |
|---|---|---|
| `-v`, `--verbose` | Off | Print extra airportd lines and compact IP tags. |
| `-l`, `--log` | Off | Write `iairport-samples.csv`, `iairport-roams.csv`, and `bssid_list.txt`. |
| `-d`, `--debug` | Off | Toggle Wi-Fi debug logging and exit. Root access is required. |
| `-i`, `--interface X` | CoreWLAN default | Use a named Wi-Fi interface. |
| `--oui PATH` | Search path | Load the OUI database from a specific path. |
| `--no-notify` | Notifications on | Skip macOS roam notifications. |
| `--no-color` | Auto | Disable ANSI color. Non-TTY output also disables color. |
| `--no-session-log` | Session log on | Skip the session log file. |
| `--log-dir PATH` | `~/Library/Logs/iairport` | Put the session log in this folder. |
| `--json` | Off | Write newline-delimited JSON. This also disables color. |
| `--interval N` | `1` | Set the sample interval in seconds. |
| `--sudo-log` | Off | Ask sudo once at startup to run `log stream` as root. |
| `-h`, `--help` | Off | Show help and exit. |

`--interval` accepts only finite positive values up to 86400. A zero, negative, non-finite, or larger value is a usage error. In non-TTY output, the status line prints once per interval.

## Session log

Each run writes a session log under `~/Library/Logs/iairport/`. The file name is `iairport-YYYYMMDD-HHMMSS.log`, with the pid added when a file already has that name. It holds the plain-text transcript: every status line, event line, and summary. With `--json`, it holds the NDJSON output. Nothing is pruned, so delete old files yourself. Use `--no-session-log` to skip it. `--log-dir PATH` puts the file in another folder, for example a case folder; the folder is created if needed.

## sudo

`log stream` refuses to run for accounts outside the `admin` group. On such an account `iairport --sudo-log` asks sudo once at startup for exactly `/usr/bin/log stream --predicate 'process == "airportd"' --info --style compact`. Only that command runs as root. iairport keeps running as you and reads its output. The flag needs a terminal on stdin and stderr; otherwise iairport prints why it skipped the prompt and runs without root. Admin accounts skip it too, since `log stream` already runs for them. Press Ctrl-C at the prompt to skip it. If sudo refuses, iairport continues without log events. The `wdutil` fields need a long-lived root process, so this prompt does not provide them. Without the flag, plain `iairport` never prompts; when `log stream` fails it prints one warning that names both `--sudo-log` and `sudo iairport`.

`sudo iairport` starts a root helper for `wdutil` and `log stream` and runs the monitor as the invoking user. `sudo iairport -d` uses that helper for `wdutil log`. Plain `iairport` after `sudo -v` calls `sudo -n wdutil` itself. If no path is available, root-only fields stay blank. `-v` shows which `wdutil` path is in use.

## Exit codes

| Code | Meaning |
|---:|---|
| 0 | Help printed, debug toggle succeeded, or the monitor shut down cleanly. |
| 1 | `-d` could not get root access. |
| 2 | Usage error. |

## Signals

Ctrl-C sends SIGINT. SIGINT, SIGTERM, and SIGHUP ask iairport to shut down. It stops the `log stream` child, waits for it, and prints the summary. If the child does not exit, iairport kills that child process and then exits.
