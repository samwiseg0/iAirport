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
| `--json` | Off | Write newline-delimited JSON. This also disables color. |
| `--interval N` | `1` | Set the sample interval in seconds. |
| `--no-sudo` | Ask when needed | Do not ask sudo for root at startup. |
| `-h`, `--help` | Off | Show help and exit. |

`--interval` accepts only finite positive values up to 86400. A zero, negative, non-finite, or larger value is a usage error. In non-TTY output, the status line prints once per interval.

## sudo

Plain `iairport` on an account that is not an admin asks sudo once at startup for exactly `/usr/bin/log stream --predicate 'process == "airportd"' --info --style compact`. Only that command runs as root. iairport keeps running as you and reads its output. It asks only when stdin and stderr are a terminal, and `--no-sudo` turns it off. Press Ctrl-C at the prompt to skip it. If sudo refuses, iairport continues without log events. The `wdutil` fields need a long-lived root process, so this prompt does not provide them.

`sudo iairport` starts a root helper for `wdutil` and `log stream` and runs the monitor as the invoking user. `sudo iairport -d` uses that helper for `wdutil log`. Plain `iairport` after `sudo -v` calls `sudo -n wdutil` itself. If no path is available, root-only fields stay blank. `-v` shows which `wdutil` path is in use.

## Exit codes

| Code | Meaning |
|---:|---|
| 0 | Help printed, debug toggle succeeded, or the monitor shut down cleanly. |
| 1 | `-d` could not get root access. |
| 2 | Usage error. |

## Signals

Ctrl-C sends SIGINT. SIGINT, SIGTERM, and SIGHUP ask iairport to shut down. It stops the `log stream` child, waits for it, and prints the summary. If the child does not exit, iairport kills that child process and then exits.
