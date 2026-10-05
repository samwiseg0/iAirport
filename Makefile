PREFIX ?= /usr/local
BINDIR ?= $(PREFIX)/bin
LIBEXECDIR ?= $(PREFIX)/libexec
DATADIR ?= $(PREFIX)/share/iairport
# Launch Services marks app bundles outside /Applications launch-disabled on
# some Macs. locationd then cannot verify the app and clears its Location
# grant, so install the bundle where Launch Services treats it as an app.
APPINSTALLDIR ?= /Applications
CODESIGN_IDENTITY ?= -
APPDIR := build/iairport.app

.PHONY: all build bundle icon test install uninstall clean

all: bundle

build:
	swift build -c release

bundle: build
	rm -rf "$(APPDIR)"
	install -d "$(APPDIR)/Contents/MacOS" "$(APPDIR)/Contents/Resources"
	install -m 0755 .build/release/iairport "$(APPDIR)/Contents/MacOS/iairport"
	install -m 0644 Resources/Info.plist "$(APPDIR)/Contents/Info.plist"
	install -m 0644 oui.txt "$(APPDIR)/Contents/Resources/oui.txt"
	install -m 0644 Resources/AppIcon.icns "$(APPDIR)/Contents/Resources/AppIcon.icns"
	codesign -s "$(CODESIGN_IDENTITY)" -f "$(APPDIR)"

# Regenerates Resources/AppIcon.icns from scripts/make-icon.swift.
icon:
	swift scripts/make-icon.swift Resources/AppIcon.icns

test:
	swift test

install:
	@test -d "$(APPDIR)" || { echo "error: $(APPDIR) not found. Run 'make' first, then 'sudo make install'." >&2; exit 1; }
	@stale=$$(find Sources Resources Package.swift oui.txt -newer "$(APPDIR)/Contents/MacOS/iairport" -type f 2>/dev/null | head -1); \
	if [ -n "$$stale" ]; then echo "error: $(APPDIR) is older than $$stale. Run 'make' first, then 'sudo make install'." >&2; exit 1; fi
	install -d "$(BINDIR)" "$(APPINSTALLDIR)" "$(DATADIR)"
	rm -rf "$(APPINSTALLDIR)/iairport.app"
	cp -R "$(APPDIR)" "$(APPINSTALLDIR)/iairport.app"
	ln -sfn "$(APPINSTALLDIR)/iairport.app/Contents/MacOS/iairport" "$(BINDIR)/iairport"
	install -m 0644 oui.txt "$(DATADIR)/oui.txt"
	@# Older installs put the bundle in $(LIBEXECDIR). Remove it so only one
	@# bundle with this identifier stays installed.
	rm -rf "$(LIBEXECDIR)/iairport.app"

uninstall:
	rm -f "$(BINDIR)/iairport" "$(DATADIR)/oui.txt"
	rm -rf "$(APPINSTALLDIR)/iairport.app" "$(LIBEXECDIR)/iairport.app"

clean:
	swift package clean
	rm -rf build
