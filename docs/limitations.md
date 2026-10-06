# Limits

## Deauth reasons

No live deauth line was observed during the capture window. The classifier matches the old style and a generic reason-code pattern. Treat deauth reason parsing as unverified until a live line is captured.

## Redacted log BSSIDs

airportd log lines redact BSSIDs without Apple's Wi-Fi logging profile. iairport gets live BSSID from CoreWLAN after Location permission. In cache mode, roam log markers can show that a roam happened before the scan cache changes.

## Location permission

The first run needs the macOS Location prompt. iairport runs in cache mode until the grant lands, then switches to live without a restart. If the dialog never appears, turn on iairport in System Settings > Privacy & Security > Location Services. If the grant is denied, iairport stays in cache mode. The grant is tied to the installed binary signature, so changed code asks again after an upgrade or `sudo make install`.

macOS keeps one Location record per app bundle id. Two installed copies with different signatures, such as a Homebrew build next to a `make install` build, share that record. While one copy is running, the other gets no prompt and stays in cache mode. Starting an older copy while the current one was live dropped both to cache mode until that run ended. Keep one install.

airportd checks the grant on every request against the bundle the running process started from. If that bundle is removed or replaced while iairport runs, the check fails and iairport drops to cache mode even though the grant is still on. `brew upgrade iairport`, `sudo make install` and a rebuild of `build/iairport.app` all do this to a copy started from that path. iairport prints a yellow line naming the likely cause. Live mode returns on its own if the bundle comes back; after an upgrade or install it will not, so restart iairport. Verified on macOS 26.5.1 by moving the bundle aside and back.

If the toggle turns itself off again within a second, locationd could not verify the app. On the verified managed Mac, locationd logged `The given bundleId or bundlePath is not a plugin or an app` and then `Clearing client authorization for verification-failed client`. Launch Services had marked the bundle `launch-disabled` at `/usr/local/libexec` and at `~/Applications`, but not in `/Applications`. That held even for an ad-hoc signed app that Gatekeeper rejects. On those Macs run `sudo make install APPINSTALLDIR=/Applications`. The Homebrew formula keeps its bundle in the Cellar, so it has the same problem there.

The notification daemon applies the same check. A `launch-disabled` bundle gets `Failed to find or validate client` from usernoted, and roam banners fall back to osascript with Script Editor's icon. iairport prints one note when that happens. Bundles under a temp directory such as `/tmp` are always `launch-disabled`.

## Protected folders

Running `build/iairport.app` from Documents, Desktop, or Downloads cannot get live SSID and BSSID. iairport prints a hint and runs in cache mode. The supported path is `sudo make install`, then `iairport`.

## Root and sudo

Root has no Location grant, so `sudo iairport` runs the monitor as the invoking user and keeps root only for `wdutil` and `log stream`.

Some managed Macs replace the sudoers policy with a third-party plugin in `/etc/sudo.conf`, such as BeyondTrust (Avecto) Defendpoint. These plugins ignore `sudo -n` and prompt on the terminal. When `/etc/sudo.conf` lists a `Plugin` that is not `sudoers_*`, plain `iairport` skips the `sudo -n` path. Every `sudo -n` call also has a timeout, and on timeout iairport kills the child and restores the terminal settings. The plugin's policy can deny `sudo iairport` outright ("user is not allowed to execute ... as root"), and it can block shells as root. On those hosts `iairport --sudo-log` asks sudo once at startup for `/usr/bin/log stream` itself, which such policies may allow. The `wdutil` fields stay blank there: polling `wdutil info` every 5 seconds would need a prompt for each call.

Some sudo policy plugins print prompt text, such as a reason menu, to stdout or stderr instead of the terminal. iairport copies both to the terminal until the first `log stream` line arrives. iairport starts sudo with `posix_spawn` and keeps it in iairport's own process group, the terminal's foreground group, so sudo can own the terminal for its prompt and Ctrl-C reaches it. Foundation's `Process` would put sudo in a new background process group, where neither works.

## CachedScanRecord

`CachedScanRecord` is an undocumented SCDynamicStore value. It worked on macOS 26.5.1 and 26.6.2. Its `BSSID` string drops leading zeros in each octet, for example `68:51:34:7c:32:1`. iairport pads those octets. It can lag the live association for minutes after a join to another AP. iairport marks cache BSSIDs with `~` and treats decode failures as missing data. In cache mode it is also the only source of AP names, so only the current AP can have one.

## log stream needs admin

`log stream` refuses to run for accounts that are not in the `admin` group. On such accounts `iairport --sudo-log` asks sudo once at startup to run `/usr/bin/log stream` as root. iairport stops it on exit, and it also dies on its next write once iairport is gone. Without root, iairport prints one warning and runs without airportd log events. Roam markers, roam reasons and join timing stay blank. BSSIDs in those lines stay redacted either way.

## CoreWLAN callbacks

CoreWLAN event registration succeeded in a plain CLI. The callbacks did not fire during the verified run. iairport still registers them, but it relies on SCDynamicStore notifications, airportd roam lines, and the poll.

## wdutil output

`wdutil info` and `wdutil log` are Apple command output. Their format may change across macOS releases. iairport parses known field names and leaves unknown values blank.

## macOS only

iairport uses CoreWLAN, SCDynamicStore, `wdutil`, `log stream`, and Darwin network counters. Those sources are macOS-only. Other operating systems are outside this tool.
