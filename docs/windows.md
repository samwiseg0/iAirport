# Windows notes

The Windows build lives in [`windows/`](../windows/README.md). This page holds the detail that does not fit the short README: how each event line is produced, what reduced mode changes, and what is still unverified.

## Reduced mode

Windows 11 24H2 and later return `ERROR_ACCESS_DENIED` from the Wi-Fi queries when Location is off for desktop apps. Microsoft lists `WlanGetNetworkBssList`, `WlanGetAvailableNetworkList` and the `current_connection` query among them. iairport keeps running with these changes:

- `bssid_source` is `connection` instead of `bss`.
- When only the BSS list is refused: RSSI in dBm, channel, band, width, AP name, FT and MFP are blank. The status line prints `Signal NN%` from the driver's signal quality instead of `RSSI -NN`. Security falls back to the auth and cipher algorithm from the connection attributes, so it reads `wpa2-personal` or `wpa3-enterprise` without the `FT` and `MFP` tags.
- When the connection query is refused too: the interface state still says connected, so the status line shows a connected link with no SSID, BSSID or AP name. RSSI and channel print when the driver answers those opcodes. JOIN and DISCONNECT still print from the interface state. If MSM notifications are refused, ACM notifications alone drive connect and disconnect, and JOIN TIMING has no `assoc` or `auth`.
- One yellow hint line names the setting to turn on. The hint prints once per run.

Turning Location on while iairport runs is enough. The next sample switches back to `bss`.

This mode is written from the Microsoft documentation. It has not been run on a 24H2 machine yet.

## Which notification drives which line

| Line | Driven by |
|---|---|
| `JOIN`, `ROAM`, `RECONNECT`, `DISCONNECT` | The poll compares the BSSID and SSID from the connection query with the last sample. MSM `connected`, `disconnected` and `roaming_end` schedule extra reads at +0, +0.5, +2 and +3 s so the change is seen inside a second. |
| `JOIN TIMING` | `assoc` is ACM connection start (or MSM roaming start) to MSM associated. MSM associating and associated arrive in the same instant on the Intel driver tested, so associating alone would always read 0 ms. `auth` is associated to connected. `linkup` is MSM connected to the poll seeing interface state connected. `ipv4` and `ipv6` are the time to the first usable address after the join. The line prints when all five are known, or after 15 s with the known parts. |
| `WLAN DISCONNECTED  reason N text` | ACM `disconnected` with its `wlanReasonCode`. The text is from `WlanReasonCodeToString`. These are WLAN_REASON_CODE values, which is why the label differs from `DEAUTH`. |
| `WLAN CONNECT FAILED  reason N text` | ACM `connection_attempt_fail`. |
| `DEAUTH  reason N text` | Event Log 8003 when its `Reason` text names a deauthentication or disassociation and carries an 802.11 reason code. The 802.11 table from `ReasonCodes.swift` supplies the text. |
| `DHCP changed`, `DHCPv6 changed` | Any event on the DHCP client and DHCPv6 client operational channels, folded into the next IP state line. |
| `-v` lines | Event Log 8000, 8001, 11000, 11001, 11004, 11005, 11010, 12011, 12012; MSM roaming start and end; MSM disconnected with its reason; link degraded and improved. |
| Red lines | Event Log 8002 (connection failed), 11002 (association failed), 11006 (security failed), 12013 (802.1X failed). |

## Join timing on Windows

Windows reports association and 802.1X or PSK completion as separate MSM notifications, so `assoc` and `auth` are measured from the service's own timestamps. `assoc` starts at the ACM connection start, because the Intel driver tested reports associating and associated together. `linkup` depends on the poll interval, so with the default 1 s interval it is accurate to about one second. On macOS all five values come from airportd log lines with their own clock.

The poll prints `JOIN` before the timing is complete. The `join` and `reconnect` rows in `iairport-roams.csv` wait for their own `JOIN TIMING`, up to 20 s, so the timing columns land on the right row.

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

Verified on Windows 11 23H2 (build 22631), Intel Dual Band Wireless-AC 8260, Aruba APs:

- `--help` exits 0, an unknown flag exits 2.
- Status line with `RSSI`, channel, width, AP name from the Aruba vendor IE, and vendor.
- `DISCONNECT` and `JOIN` with `JOIN TIMING` and `IP` lines, via `netsh wlan disconnect` and `connect`.
- Roam between two APs of the same SSID: `ROAM` with channel and RSSI change, then `IP after roam  v4 kept  v6 kept` one second later. Two roams in a row, 116 to 112 and back.
- `-l` CSV rows and `--json` objects match `docs/output.md`.
- Ctrl-C prints the summary and closes the session log.

Still open:

1. Windows 11 24H2 with Location off: the hint line, `bssid_source=connection`, blank or partial link fields. Turn Location on and watch the next sample switch to `bss`.
2. Turn Wi-Fi off and on: `DISCONNECT`, then `JOIN` with `JOIN TIMING`.
3. Power off the current AP: `DISCONNECT`, `WLAN DISCONNECTED  reason N`, and a `DEAUTH` line from Event Log 8003 if the controller sent one.
4. Forget and rejoin a WPA2-Enterprise network: 12011 and 12012 in `-v`, and `auth` in `JOIN TIMING`.
5. Renew the lease (`ipconfig /renew`): `DHCP changed` and an `IP` line.
6. Ctrl-Break and closing the window: the summary prints and the session log closes.
7. Run as a standard user with the Event Log channel locked down by policy: one warning, the monitor keeps running.
