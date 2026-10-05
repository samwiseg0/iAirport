PREFIX ?= /usr/local
BINDIR ?= $(PREFIX)/bin
LIBEXECDIR ?= $(PREFIX)/libexec
DATADIR ?= $(PREFIX)/share/iairport
# Where the app bundle goes. Some managed Macs mark bundles outside
# /Applications launch-disabled in Launch Services, and locationd then drops
# the Location grant. Pass APPINSTALLDIR=/Applications on those Macs.
APPINSTALLDIR ?= $(LIBEXECDIR)
# Trailing slashes would break the same-directory check below.
override APPINSTALLDIR := $(patsubst %/,%,$(APPINSTALLDIR))
override LIBEXECDIR := $(patsubst %/,%,$(LIBEXECDIR))
APPLICATIONSDIR := /Applications
CODESIGN_IDENTITY ?= -
SWIFT_BUILD_FLAGS ?=
APPDIR := build/iairport.app

.PHONY: all build bundle icon test install uninstall clean

all: bundle

build:
	swift build -c release $(SWIFT_BUILD_FLAGS)

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
	install -d "$(BINDIR)" "$(DATADIR)"
	@# install -d resets the mode of an existing folder. /Applications is not ours.
	@test -d "$(APPINSTALLDIR)" || install -d "$(APPINSTALLDIR)"
	rm -rf "$(APPINSTALLDIR)/iairport.app"
	cp -R "$(APPDIR)" "$(APPINSTALLDIR)/iairport.app"
	ln -sfn "$(APPINSTALLDIR)/iairport.app/Contents/MacOS/iairport" "$(BINDIR)/iairport"
	install -m 0644 oui.txt "$(DATADIR)/oui.txt"
	@# One bundle with this identifier at a time. Drop copies at the other
	@# install locations this Makefile has used.
	@for dir in "$(LIBEXECDIR)" "$(APPLICATIONSDIR)"; do \
	  if [ "$$dir" != "$(APPINSTALLDIR)" ] && [ -d "$$dir/iairport.app" ]; then \
	    echo "rm -rf $$dir/iairport.app"; rm -rf "$$dir/iairport.app"; \
	  fi; \
	done

uninstall:
	rm -f "$(BINDIR)/iairport" "$(DATADIR)/oui.txt"
	rm -rf "$(APPINSTALLDIR)/iairport.app" "$(LIBEXECDIR)/iairport.app" "$(APPLICATIONSDIR)/iairport.app"

clean:
	swift package clean
	rm -rf build
