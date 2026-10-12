#!/bin/sh
set -eu
lqm='LQM: rssi=-?[0-9]+dBm per_ant_rssi=\(-?[0-9]+dBm(, -?[0-9]+dBm)*\)( noise=-?[0-9]+dBm)?( snr=[0-9]+)? cca=[0-9.]+% ccaSelfTotal=[0-9]+ ccaOtherTotal=[0-9]+ interferenceTotal=[0-9]+ txRate=[0-9.]+Mbps txFrames=[0-9]+ txFail=[0-9]+ txRetrans=[0-9]+ txFallbackRate=[0-9.]+Mbps rxRate=[0-9.]+Mbps rxFrames=[0-9]+ rxRetryFrames=[0-9]+ rxToss=[0-9]+( beaconRecv=[0-9]+ beaconSched=[0-9]+)? +txFwFrames=[0-9]+ txFwFail=[0-9]+ txFwRetrans=[0-9]+ network = .* bssid = .* channel = [0-9]+ BW = [0-9]+ band = [0-9]+'
roam='Roam: <airport\[[0-9]+\]> .*(BEST CONNECTED ROAM triggered|Requesting Roam : \{|Found (matching scan result|one matching scan result|more than three matching scan results)|Network : \[.*\]\. Num BSSID : \[[0-9]+\].*Roaming profile type : \[[^]]+\]|ROAMING PROFILES updated to [A-Z-]+ for [[:alnum:]]+)'
dhcp='(CWFEventTypeDHCPChanged|Processing DHCP: '\''State:/Network/Service/[0-9A-F-]+/DHCP'\''|_processDHCPChanges: (State:/Network/Service/[0-9A-F-]+/DHCP|DHCP airport_changed = [0-9]+, service:[0-9A-F-]+)|_setDHCPMessage: dhcpInfoKey "State:/Network/Interface/[^/]+/AirPort/DHCP Message" = |IPConfigLatency|DHCP latency|lease)'
link='(Received link change event - linkDown \([0-9]+\), debounce \([0-9]+\)|AUTO-JOIN: (Will run request|Auto-join triggered|Updated join status|Updated associated network|Auto-join aborted|Scheduling auto-join retry)|Link (UP|DOWN)|Disconnected|disconnect)'
deauth='Received (Deauth|Disassoc) from (<redacted>|([0-9A-Fa-f]{2}:){5}[0-9A-Fa-f]{2}) with Reason [0-9]+'
printf 'lqm_count=%s\n' "$(grep -E -c "$lqm" iairport-research/airportd_0854_0856.log)"
printf 'roam_count=%s\n' "$(grep -E -c "$roam" iairport-research/roam_debug_matches.log)"
printf 'dhcp_count=%s\n' "$(grep -E -c "$dhcp" iairport-research/dhcp_matches_info.log)"
printf 'link_count=%s\n' "$(grep -E -c "$link" iairport-research/link_assoc_matches.log)"
printf '%s\n' '2026-10-05 00:00:00.000 Df airportd[483:abcd] [com.apple.WiFiManager:] [corewifi] Received Deauth from d0:4d:c6:b2:78:40 with Reason 4' > iairport-research/deauth_synthetic.log
printf 'deauth_synthetic_count=%s\n' "$(grep -E -c "$deauth" iairport-research/deauth_synthetic.log)"
