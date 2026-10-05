# iairport

iairport watches a Mac Wi-Fi link while you roam, disconnect, and join again. It prints one live status line, event lines, IP state, and optional CSV or JSON logs. It is a Swift rewrite of fllthblnks' 2015 Perl [iAirport](legacy/iAirport.pl). The old `airport` and `wifi.log` data are gone from current macOS.

- **Live status line.** It shows SSID, BSSID, AP name, vendor, channel, rate, RSSI, noise, SNR, CCA, speed, and bytes.
- **Roams.** It shows the old and new BSSID, AP name, SSID, channel change, and RSSI change.
- **Disconnects and reconnects.** It shows link changes and reason text where the log gives one.
- **Join timing.** It prints association, auth, link, IPv4, and IPv6 timing from airportd.
- **IP state.** It tracks IPv4, IPv6, routers, and IPv6 address kinds.
- **Logs.** It writes CSV files with headers or newline JSON for scripts.
- **Root extras.** Root access adds MCS, NSS, guard interval, and `-d` debug logging.

[docs/features.md](docs/features.md) says what each line means. [docs/output.md](docs/output.md) lists the CSV and JSON fields. [docs/how-it-works.md](docs/how-it-works.md) explains the data sources.

## Requirements

- macOS 14 or later. The current build was verified on macOS 26.5.1 arm64.
- Location permission for iairport. macOS asks on first run.
- Swift 5.9 or Xcode Command Line Tools to build. Homebrew builds from source and needs them too.
- No root for the core monitor.
- Root access for MCS, NSS, guard interval, and `-d` debug logging.

## Install

Install with Homebrew:

```sh
brew install samwiseg0/tap/iairport
```

The formula builds from source, so it needs the Xcode Command Line Tools. Update with `brew upgrade iairport`.

Or build and install from a clone:

```sh
make
sudo make install
```

`make` builds `build/iairport.app`. `sudo make install` installs that existing bundle to `/usr/local/libexec/iairport.app`, plus `/usr/local/bin/iairport` and `/usr/local/share/iairport/oui.txt`. Set `PREFIX=/path` for another prefix.

Some managed Macs mark app bundles outside `/Applications` launch-disabled. The Location toggle then turns itself off within a second of Allow. On those Macs install the bundle into `/Applications`:

```sh
sudo make install APPINSTALLDIR=/Applications
```

`make install` removes the bundle from the other location it has used, so one copy stays installed. A Homebrew install keeps its bundle in the Cellar and hits the same problem on such Macs. Use `make install APPINSTALLDIR=/Applications` there instead.

Run the installed command, not the app from a repo clone under Documents, Desktop, or Downloads. Those protected folders stay in cache mode. The first run asks for Location permission. Click Allow. The grant is tied to the installed binary signature, so changed code asks again after an upgrade or `sudo make install`.

Keep one install. macOS holds one Location record per app, and two copies with different signatures fight over it. If an older copy is still running, the new one gets no prompt. Quit it and run again. Switching from a source install to Homebrew, run `sudo make uninstall` first.

You can also build with SwiftPM, but the bare binary runs in cache mode:

```sh
swift build -c release
```

## Usage

Run the monitor:

```sh
iairport
```

Write CSV logs:

```sh
iairport -l
```

Write JSON to a file:

```sh
iairport --json > iairport-events.ndjson
```

Toggle Wi-Fi debug logging:

```sh
sudo iairport -d
```

`sudo iairport` keeps root for `wdutil` and `log stream` and runs the monitor as your user, because root has no Location grant. On an account that is not an admin, `iairport --sudo-log` asks sudo once at startup to run `/usr/bin/log stream` for airportd as root. Nothing else runs as root. Without the flag, iairport never prompts.

Write plain non-TTY output:

```sh
iairport --no-color --no-notify | tee iairport-status.log
```

See [docs/commands.md](docs/commands.md) for every flag and exit code.

## Credits and licence

Original iAirport was written by Guillaume Germain, fllthblnks, in 2015. This repository is a fork of `fllthblnks/iAirport` and now lives at `github.com/samwiseg0/iAirport`. The Swift rewrite keeps the original terminal workflow and OUI lookup. The project keeps the MIT licence, see [LICENSE.md](LICENSE.md).
