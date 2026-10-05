# Limits

## Deauth reasons

No live deauth line was observed during the capture window. The classifier matches the old style and a generic reason-code pattern. Treat deauth reason parsing as unverified until a live line is captured.

## Redacted log BSSIDs

airportd log lines redact BSSIDs without Apple's Wi-Fi logging profile. iairport gets live BSSID from CoreWLAN after Location permission. In cache mode, roam log markers can show that a roam happened before the scan cache changes.

## Location permission

The first run needs the macOS Location prompt. iairport runs in cache mode until the grant lands, then switches to live without a restart. If the dialog never appears, turn on iairport in System Settings > Privacy & Security > Location Services. If the grant is denied, iairport stays in cache mode. The grant is tied to the installed binary signature, so changed code asks again after `sudo make install`.

## Protected folders

Running `build/iairport.app` from Documents, Desktop, or Downloads cannot get live SSID and BSSID. iairport prints a hint and runs in cache mode. The supported path is `sudo make install`, then `iairport`.

## Root and sudo

Root has no Location grant, so `sudo iairport` runs the monitor as the invoking user and keeps root only for `wdutil` and `log stream`.

## CachedScanRecord

`CachedScanRecord` is an undocumented SCDynamicStore value. It worked on macOS 26.5.1 and 26.6.2. Its `BSSID` string drops leading zeros in each octet, for example `68:51:34:7c:32:1`. iairport pads those octets. It can lag the live association for minutes after a join to another AP. iairport marks cache BSSIDs with `~` and treats decode failures as missing data. In cache mode it is also the only source of AP names, so only the current AP can have one.

## log stream needs admin

`log stream` refuses to run for accounts that are not in the `admin` group. On such accounts iairport prints one warning and runs without airportd log events. Roam markers, roam reasons and join timing stay blank. `sudo iairport` fixes this: the root helper runs `log stream` and passes its output to the monitor. BSSIDs in those lines stay redacted either way.

## CoreWLAN callbacks

CoreWLAN event registration succeeded in a plain CLI. The callbacks did not fire during the verified run. iairport still registers them, but it relies on SCDynamicStore notifications, airportd roam lines, and the poll.

## wdutil output

`wdutil info` and `wdutil log` are Apple command output. Their format may change across macOS releases. iairport parses known field names and leaves unknown values blank.

## macOS only

iairport uses CoreWLAN, SCDynamicStore, `wdutil`, `log stream`, and Darwin network counters. Those sources are macOS-only. Other operating systems are outside this tool.
