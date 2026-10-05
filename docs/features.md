# What it does

iairport reads the Wi-Fi link and prints what changes. The live line gives the current association. Event lines show joins, roams, disconnects, IP changes, and airportd messages. [output.md](output.md) shows the exact text and log fields.

## The status line

The status line prints once per sample interval. On a TTY it rewrites the same line. In a pipe it prints one line per interval. It shows SSID, BSSID, vendor, channel, PHY, security, rate, RSSI, noise, SNR, CCA, speed, and data. A `~` after the BSSID means cache mode, so the value can lag. Verbose mode adds antenna RSSI, retry counters, and compact IP tags.

## Roams

A roam is a change from one associated BSSID to another. iairport prints the old and new BSSID, AP name, SSID, channel change, and RSSI change. In cache mode, a driver roam can count with `?` as the new BSSID. iairport records the dwell time and keeps a roam history for the exit summary.

## AP names

Many enterprise APs put their name in a vendor element of the beacon. iairport decodes Aruba and HPE, Cisco and Meraki, Juniper Mist, Ubiquiti, Ruckus, Extreme and Aerohive, Fortinet, Arista, Huawei, Alcatel-Lucent, Belden, Meter, and Telecom Infra Project layouts. The name prints after the BSSID in the status line, event lines, and summary table, and goes into CSV and JSON as `ap_name`. APs without a name element show only the BSSID. Only Aruba was checked against live beacons. The other layouts follow the Wireshark dissector.

## Disconnects and reconnects

A disconnect is an associated link going down. A reconnect is a return to the same BSSID after a down state. iairport prints the last BSSID and SSID. Reason text appears where the airportd line gives one.

## Join timing

airportd logs an `AUTO-JOIN: Updated join status` line after a join. iairport reads the timing fields from that line. It can print `assoc`, `auth`, `linkup`, `ipv4`, `ipv6`, `ipv4Primary`, and `ipv6Primary`. The same values can appear in the roam CSV when they land near the transition.

## IP addresses

iairport tracks IPv4 and IPv6 with SCDynamicStore. It shows the first IPv4 address and router. For IPv6, it prefers a non-deprecated global SLAAC or DHCPv6 address. If no such address exists, it shows the first global address. If only link-local exists, it prints `v6 none`.

IPv6 flags come from the system. `0x0002` means tentative. `0x0010` means deprecated. `0x0040` means autoconf, used for SLAAC. `0x0080` means temporary. `0x0100` means dynamic, used for DHCPv6. `0x0400` means secured.

## Data counters

The `Speed:` field is bits per second for the last interval. The `Data:` field is bytes since iairport started. The summary prints the same byte total as `bytes`. The counters are 64-bit and handle wrap by rebasing the delta.

## Vendor lookup

iairport looks up the first three octets of the BSSID in `oui.txt`. The search starts with `--oui`. Then it tries app bundle `Resources/oui.txt`. After that it checks the current directory, executable directory, SwiftPM layout, and `/usr/local/share/iairport/oui.txt`. If the file is missing, it warns once. Vendor names are blank when there is no match.

## Notifications

Roam notifications have the title `iAirport Roam`. The text is the old and new AP name, or the BSSID when there is no name, plus the channel change. `--no-notify` turns this off.

When iairport runs from its app bundle it posts through UserNotifications, so the banner carries the iairport icon. The first roam asks for Notifications permission once. Roams that arrive while the prompt is up collapse into the latest one. Deny it and iairport posts nothing and prints one note in the terminal. A plain `.build/release/iairport` has no bundle, so it falls back to `/usr/bin/osascript` and `display notification`. That banner shows Script Editor's icon. When macOS rejects the bundle, iairport prints one note, switches to osascript for the rest of the run, and keeps the banners to one look.

## Debug logging as root

`-d` reads `wdutil log` and toggles Wi-Fi logging with `wdutil log +wifi` or `wdutil log -wifi`. Root access is required, so run `sudo iairport -d`. `sudo iairport` and a run after `sudo -v` can also read MCS, NSS, and guard interval from `wdutil info`. Without root access, those fields stay blank. `-v` shows which `wdutil` path is in use.
