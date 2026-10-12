# Windows notes

The Windows build lives in [`windows/`](../windows/README.md). This page holds the detail that does not fit the short README: how each event line is produced, what reduced mode changes, and what is still unverified.

## Reduced mode

`WlanGetNetworkBssList` returns `ERROR_ACCESS_DENIED` on Windows 11 24H2 and later when Location is off for desktop apps. The connection query still works, so iairport keeps running with these changes:

- `bssid_source` is `connection` instead of `bss`.
- RSSI in dBm, channel, band, width, AP name, FT and MFP are blank. The status line prints `Signal NN%` from the driver's signal quality instead of `RSSI -NN`.
- Security falls back to the auth and cipher algorithm from the connection attributes, so it reads `wpa2-personal` or `wpa3-enterprise` without the `FT` and `MFP` tags.
- One yellow hint line names the setting to turn on. The hint prints once per run.

Turning Location on while iairport runs is enough. The next BSS list refresh succeeds and the sample switches back to `bss`.

## Which notification drives which line

| Line | Driven by |
|---|---|
| `JOIN`, `ROAM`, `RECONNECT`, `DISCONNECT` | The poll compares the BSSID and SSID from the connection query with the last sample. MSM `connected`, `disconnected` and `roaming_end` schedule extra reads at +0, +0.5, +2 and +3 s so the change is seen inside a second. |
| `JOIN TIMING` | `assoc` is MSM associating to associated. `auth` is associated to connected. `linkup` is MSM connected to the poll seeing interface state connected. `ipv4` and `ipv6` are the time to the first usable address after the join. The line prints when all five are known, or after 15 s with the known parts. |
| `WLAN DISCONNECTED  reason N text` | ACM `disconnected` with its `wlanReasonCode`. The text is from `WlanReasonCodeToString`. These are WLAN_REASON_CODE values, which is why the label differs from `DEAUTH`. |
| `WLAN CONNECT FAILED  reason N text` | ACM `connection_attempt_fail`. |
| `DEAUTH  reason N text` | Event Log 8003 when its `Reason` text names a deauthentication or disassociation and carries an 802.11 reason code. The 802.11 table from `ReasonCodes.swift` supplies the text. |
| `DHCP changed`, `DHCPv6 changed` | Any event on the DHCP client and DHCPv6 client operational channels, folded into the next IP state line. |
| `-v` lines | Event Log 8000, 8001, 11000, 11001, 11004, 11005, 11010, 12011, 12012; MSM roaming start and end; MSM disconnected with its reason; link degraded and improved. |
| Red lines | Event Log 8002 (connection failed), 11002 (association failed), 11006 (security failed), 12013 (802.1X failed). |

## Join timing on Windows

Windows reports association and 802.1X or PSK completion as separate MSM notifications, so `assoc` and `auth` are measured from the service's own timestamps. `linkup` depends on the poll interval, so with the default 1 s interval it is accurate to about one second. On macOS all five values come from airportd log lines with their own clock.

## Event Log IDs used

| Event ID | Meaning | iairport line |
|---:|---|---|
| 8000 | Connection start | `-v` |
| 8001 | Connected | `-v` |
| 8002 | Connection failed | red `WLAN connection failed` |
| 8003 | Disconnected | `DEAUTH` when the reason text has a deauth or disassoc code, else yellow `WLAN disconnected` |
| 11000, 11001 | Association start and success | `-v` |
| 11002 | Association failed | red |
| 11004, 11005 | Security start and success | `-v` |
| 11006 | Security failed | red |
| 11010 | Reconnect | `-v` |
| 12011, 12012 | 802.1X start and success | `-v` |
| 12013 | 802.1X failed | red |

The XML scanner reads `EventID`, `Channel`, `Provider Name`, `TimeCreated SystemTime` and every `Data Name="..."` pair. It does not parse the rest. The unit tests use XML written by hand in the shape Windows emits; no live capture was available when the port was written.

## Fields with no Windows source

MCS, NSS, guard interval, noise floor, SNR and CCA have no public wlanapi field. The CSV columns exist and stay blank. The status line skips them. The JSON sample omits them. iairport does not estimate them from the rate.

## Still unverified

The port was cross-built and unit-tested on macOS. Nobody has yet run it against a live Windows adapter. Expect to check at least these on a real machine, in this order:

1. `iairport --help` and an unknown flag (exit 0 and 2).
2. Connected on Windows 10 or Windows 11 before 24H2: status line with `RSSI`, channel, width, AP name and vendor.
3. Windows 11 24H2 with Location off: the hint line, `Signal NN%`, `bssid_source=connection`. Turn Location on and watch the next sample switch to `bss`.
4. Roam between two APs of the same SSID: `ROAM` line with dwell, channel change and RSSI change, then `IP after roam`.
5. Turn Wi-Fi off and on: `DISCONNECT`, then `JOIN` with `JOIN TIMING`.
6. Power off the current AP: `DISCONNECT`, `WLAN DISCONNECTED  reason N`, and a `DEAUTH` line from Event Log 8003 if the controller sent one.
7. Forget and rejoin a WPA2-Enterprise network: 12011 and 12012 in `-v`, and `auth` in `JOIN TIMING`.
8. Renew the lease (`ipconfig /renew`): `DHCP changed` and an `IP` line.
9. `-l` and `--json`: CSV headers and JSON keys match `docs/output.md`; the JSON sample carries `signal_pct` next to `rssi_dbm`.
10. Ctrl-C, Ctrl-Break and closing the window: the summary prints and the session log closes.
11. Run as a standard user with the Event Log channel locked down by policy: one warning, the monitor keeps running.
