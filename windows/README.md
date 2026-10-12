# iairport for Windows

This is the Windows build of iairport, written in Rust. It watches one Wi-Fi adapter while you roam, disconnect and join again. It prints the same live status line, JOIN, ROAM, RECONNECT, DISCONNECT and DEAUTH lines, IP state, JOIN TIMING, session log, CSV and JSON as the macOS tool. Where Windows has no number for a field, the field stays blank.

[docs/windows.md](../docs/windows.md) has the longer notes. [docs/output.md](../docs/output.md) lists the CSV and JSON fields.

## Requirements

- Windows 10 1809 or later, or Windows 11.
- No admin rights. Everything reads through wlanapi, IP Helper and the Event Log as a normal user.
- On Windows 11 24H2 and later, Location must be on for RSSI in dBm, channel, width, security detail and AP name. Turn on **Settings > Privacy & security > Location**, then **Location services** and **Let desktop apps access your location**. Without it iairport still runs and prints one hint. See "Reduced mode" below.
- Event Log access to `Microsoft-Windows-WLAN-AutoConfig/Operational` for DEAUTH reasons and 802.1X detail. Standard users have it by default. If a policy blocks it, iairport prints one warning and keeps running without those lines.

## Install

Download `iairport-windows-x64.zip` from the latest [release](https://github.com/samwiseg0/iAirport/releases). It holds `iairport.exe`, `oui.txt` and the licence. Unzip it anywhere and run the exe from that folder. Nothing else is needed.

Every push to master also builds the same zip as a CI artifact on the Actions tab. Artifacts need a GitHub login and expire after 90 days.

## Build

On Windows with a Rust toolchain (1.85 or later):

```powershell
cd windows
cargo build --release
.\target\release\iairport.exe --help
```

Cross-build from macOS with Homebrew's mingw-w64:

```sh
brew install rustup mingw-w64
rustup target add x86_64-pc-windows-gnu
make windows
```

The exe lands at `windows/target/x86_64-pc-windows-gnu/release/iairport.exe`. `make windows-test` runs the unit tests on the Mac. The crate's `.cargo/config.toml` already points the gnu target at `x86_64-w64-mingw32-gcc`.

Put `oui.txt` from the repo root next to the exe, or in `%LOCALAPPDATA%\iairport\`, for vendor names.

## Usage

Run the monitor:

```powershell
iairport
```

Each run also writes a transcript to `%LOCALAPPDATA%\iairport\logs\`. Use `--log-dir PATH` to put it elsewhere, or `--no-session-log` to skip it.

Write CSV logs:

```powershell
iairport -l
```

Write JSON to a file:

```powershell
iairport --json > iairport-events.ndjson
```

Write plain output to a file and the console:

```powershell
iairport --no-color | Tee-Object iairport-status.log
```

Pick an adapter by GUID or by part of its description:

```powershell
iairport -i "Intel(R) Wi-Fi 6E"
```

Flags: `-v`, `-l`, `-i X`, `--oui PATH`, `--no-color`, `--no-session-log`, `--log-dir PATH`, `--json`, `--interval N`, `-h`. The macOS-only `-d`, `--sudo-log` and `--no-notify` are not accepted. Exit codes: 0 after help or a clean shutdown, 1 when no Wi-Fi interface is found, 2 for a usage error. Ctrl-C, Ctrl-Break and closing the console window print the summary first.

## Where each value comes from

| Value | Source | Admin |
|---|---|---|
| SSID, BSSID, PHY, Tx rate, signal % | `WlanQueryInterface` current connection | No |
| RSSI dBm, channel, band | `WlanGetNetworkBssList` entry for the connected BSSID | No, Location on 24H2 |
| AP name, width, security, FT, MFP | Beacon IEs from the same BSS entry | No, Location on 24H2 |
| Radio on or off | `WlanQueryInterface` radio state | No |
| JOIN, ROAM, RECONNECT, DISCONNECT | ACM and MSM notifications plus the poll | No |
| JOIN TIMING | ACM connection start, MSM associated and connected, then the first usable address | No |
| WLAN reason codes | `WLAN_CONNECTION_NOTIFICATION_DATA` and `WlanReasonCodeToString` | No |
| 802.11 DEAUTH reasons, 802.1X detail | Event Log `Microsoft-Windows-WLAN-AutoConfig/Operational` | No |
| DHCP and DHCPv6 changes | Event Log `Microsoft-Windows-Dhcp-Client/Operational` and `Dhcpv6-Client/Operational` | No |
| IPv4 and IPv6 addresses, routers, kinds | `GetAdaptersAddresses` with gateways and prefixes | No |
| Primary interface | `GetBestInterfaceEx` | No |
| Bytes in and out | `GetIfEntry2` InOctets and OutOctets | No |
| Retry and fail counters | `WlanQueryInterface` statistics, when the driver answers | No |
| MCS, NSS, guard interval, noise, SNR, CCA | Not available on Windows | |

## Differences from macOS

- No noise floor, SNR, CCA, MCS, NSS or guard interval. Those columns stay blank and the status line skips them.
- `bssid_source` is `bss` or `connection`. There is no scan cache and no `~` marker. `connection` means the BSS list was refused and the sample came from the connection attributes only.
- The JSON sample carries `signal_pct`, the driver's signal quality percentage. In reduced mode the status line prints `Signal 90%` where macOS prints `RSSI -61`.
- `WLAN DISCONNECTED  reason N text` and `WLAN CONNECT FAILED  reason N text` come from the WLAN service and use the WLAN_REASON_CODE namespace. `DEAUTH  reason N text` comes from the Event Log and uses 802.11 reason codes, as on macOS.
- IPv6 kinds come from the address origin fields, so the JSON `ip` object carries `prefix_origin` and `suffix_origin` instead of macOS `flags`.
- The retry columns (`tx_retrans`, `tx_fail`, `rx_retry`) are per-interval deltas of the driver's MAC counters, and `rx_retry` is the duplicate-frame count. Many drivers refuse the statistics opcode; then the columns stay blank.
- The session log and OUI file live under `%LOCALAPPDATA%\iairport\`.
