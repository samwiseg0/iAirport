# How it works

This page names the sources iairport reads. The old Perl tool used one command and one log file. The Swift tool reads several macOS APIs because those old sources no longer carry the needed fields.

## Why the old tool stopped

The old tool read `airport -I` and tailed `/var/log/wifi.log`. The `airport` binary is gone on macOS 14.4 and later. `/var/log/wifi.log` now holds airportd health checks for this workflow. `sudo wdutil info` is useful, but it redacts SSID and BSSID. `ipconfig getsummary`, `networksetup`, `system_profiler`, and `scutil` expose pieces, not the old full feed.

## Location permission

macOS treats Wi-Fi SSID and BSSID as location data. iairport ships as `iairport.app`, and the command is a symlink to its binary. At startup, iairport resolves the symlink and re-runs the real app path so macOS sees the bundle. On first run, click Allow at the Location prompt. iairport then runs `open -g -j` once as a handshake and continues in the same process. The grant is tied to the installed binary signature, so changed code asks again after `sudo make install`. Root has no Location grant, so `sudo iairport` re-runs itself as the invoking user.

## CoreWLAN

CoreWLAN is the live source for SSID and BSSID when Location permission is granted. iairport also reads RSSI, noise, Tx rate, channel, width, PHY, and security there. Live BSSID changes within about one second after a join on the verified host. If permission is missing, iairport switches to cache mode.

## Scan cache fallback

SCDynamicStore holds `State:/Network/Interface/en0/AirPort` on the verified host. Its `CachedScanRecord` can hold an unredacted BSSID and SSID with no root and no Location prompt. It is a fallback only. A verified join to another AP of the same network left this record on the old AP for minutes. In cache mode, iairport marks the BSSID with `~` and sets `bssid_source` to `cache`.

## airportd log stream

iairport runs `log stream --predicate 'process == "airportd"' --info --style compact`. LQM lines arrive about every 5 seconds and carry CCA, SNR, retry counts, rates, width, and band. Roam markers include `BEST CONNECTED ROAM triggered`, `Requesting Roam : {`, and `APPLE80211_M_ROAMED`. `AUTO-JOIN` lines carry join timing and FT data. DHCP, DHCPv6, and IPv6 RA/SLAAC changes also appear there. BSSIDs in these log lines are redacted.

## getifaddrs counters

iairport reads the `AF_LINK` record for the Wi-Fi interface with `getifaddrs`. It uses `ifi_ibytes` and `ifi_obytes`. The displayed data count starts at zero when the tool starts. If a counter wraps or moves backward, iairport rebases the delta.

## wdutil through sudo

`wdutil info` adds fields that CoreWLAN does not always expose. iairport reads MCS, NSS, guard interval, CCA, PHY, and security there. `wdutil` needs root, and root has no Location grant. `sudo iairport` keeps root as a helper for `wdutil info` and `wdutil log`. It runs the monitor as the invoking user. The monitor sends `info` or `log` requests over a socket. Plain `iairport` after `sudo -v` calls `sudo -n wdutil info` directly. Without either path, those fields stay blank. The `sudo -n` probe runs off the state queue with stdin from `/dev/null` and a 3-second timeout. It is skipped when `/etc/sudo.conf` loads a plugin that is not `sudoers_*`, because such plugins can ignore `-n` and prompt.

The split exists because sudo 1.9.14 and later run each command in a new pty and tie the sudo ticket to the tty. A user process started by root cannot reuse the ticket.

## IPv4 and IPv6 state

SCDynamicStore keys under `State:/Network/Interface/<if>/IPv4` and `.../IPv6` hold addresses, masks, prefixes, and IPv6 flags. Global IPv4 and IPv6 keys say which interface is primary. Service keys can hold routers and DHCP state. Link-local addresses use the `fe80:` prefix. Deprecated, temporary, DHCPv6, and SLAAC kinds come from flag bits. Static is the fallback.

## Source table

| Value | Source | Root |
|---|---|---|
| Live SSID and BSSID | CoreWLAN in `iairport.app` with Location permission | No |
| Cache SSID and BSSID | SCDynamicStore `CachedScanRecord` | No |
| AP name | Beacon vendor element from the CoreWLAN scan cache, or from `CachedScanRecord` in cache mode | No |
| RSSI, noise, Tx rate | CoreWLAN | No |
| Channel, width, PHY, security | CoreWLAN | No |
| Roam markers | airportd unified log | No |
| LQM, CCA, retry counters | airportd unified log | No |
| Join timing and FT | airportd unified log | No |
| DHCP and DHCPv6 changes | airportd unified log and SCDynamicStore | No |
| Bytes in and out | `getifaddrs` | No |
| MCS, NSS, guard interval | `wdutil info` through the root helper or `sudo -n` | Yes |
| IPv4 and IPv6 addresses | SCDynamicStore IPv4 and IPv6 keys | No |
