# Development

## Build

Use the Makefile for normal work:

```sh
make
```

The default `all` target runs `bundle`, which creates `build/iairport.app`. The `build` target only runs `swift build -c release`. The app bundle holds the binary, `Info.plist`, and `Resources/oui.txt`. `CODESIGN_IDENTITY` defaults to `-`, which means ad-hoc signing.

## Test

Run the Swift tests with:

```sh
swift test
```

The tests cover pure logic in `IAirportCore`. They do not need root. They should not need a live Wi-Fi roam.

## Makefile targets

- `all` runs `bundle`.
- `build` runs `swift build -c release`.
- `bundle` creates `build/iairport.app`, including `Resources/AppIcon.icns`.
- `icon` regenerates `Resources/AppIcon.icns` from `scripts/make-icon.swift` (CoreGraphics and `iconutil`, no other tools).
- `test` runs `swift test`.
- `install` installs an existing app bundle, command symlink, and OUI file.
- `uninstall` removes the installed app bundle, symlink, and OUI file.
- `clean` removes build output.

`make install` uses `PREFIX ?= /usr/local`. It expects `build/iairport.app` to exist, so run `make` first. It writes the app to `/Applications/iairport.app` (`APPINSTALLDIR`) and removes an older `/usr/local/libexec/iairport.app`. Launch Services marks bundles outside `/Applications` launch-disabled on some Macs; locationd then fails to verify the app and clears its Location grant. It symlinks `/usr/local/bin/iairport` to the app binary. It also writes `/usr/local/share/iairport/oui.txt`. Run `sudo make install` after each build when you need to test live mode. A bundle run from a repo clone under Documents, Desktop, or Downloads stays in cache mode.

## OUI refresh

Run `scripts/update-oui.sh` from the repo. It downloads `https://standards-oui.ieee.org/oui/oui.csv`. It writes `oui.txt` with source and date comments. It formats each row as `XX:XX:XX<TAB>ShortName<TAB>Organization Name`. It overrides the Aruba OUI listed in the script to `Aruba` and `Aruba, a Hewlett Packard Enterprise Company`.

## Repo layout

- `Sources/IAirportCore` holds the monitor, readers, parsers, output, and CSV logic.
- `Sources/iairport` holds the executable entry point.
- `Tests/IAirportCoreTests` holds pure logic tests.
- `legacy/iAirport.pl` is the original Perl tool.
- `docs` and `scripts` hold user docs and maintenance scripts.
