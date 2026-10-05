# Output

iairport writes human output by default. It writes JSON when `--json` is set. It writes CSV files when `-l` is set. Timestamps in JSON `ts` and CSV `ts_iso` are local ISO 8601 time with a UTC offset.

## The status line

When Wi-Fi is off or disconnected, the status line is short:

```text
time  Wi-Fi is off
time  Not connected
```

A connected line shows fields in this order:

```text
time  "ssid" (vendor bssid apname) Chan channel/width phy security FT MFP MCS mcs NSS nss (rate Mbps)  RSSI rssi NF noise SNR snr CCA cca%  Speed: ratebps Data: bytes
```

Unknown optional fields are omitted. An unknown channel prints as `Chan ?`. `Speed:` and `Data:` always print. `NF` is noise floor. `Speed:` is input plus output bits per second for the last interval. `Data:` is bytes since iairport started. A `~` after the BSSID means `bssid_source=cache`. `apname` only prints when the beacon carries one. In a terminal the status line is clipped to the window width so it can redraw in place. Widen the window, or use `-l` or `--json` for the full values. Non-TTY mode prints one status line per interval.

## Event lines

Examples use documentation addresses and values. JOIN and ROAM always include `ch` and `RSSI` labels. Unknown values after those labels are blank. An AP name follows the BSSID in brackets when the beacon carries one.

```text
2026/10/05 09:00:01  JOIN  00:0b:86:11:22:01 (AP-Lobby-01)  "ExampleNet"  ch 44  RSSI -53
2026/10/05 09:00:05  ROAM  00:0b:86:11:22:01 (AP-Lobby-01) -> 00:0b:86:11:22:02 (AP-Lobby-02)  "ExampleNet"  ch 44 -> 149  RSSI -72 -> -51
2026/10/05 09:00:06  ROAM  00:0b:86:11:22:01~ -> ?  "ExampleNet"  ch 44 ->   RSSI -72 ->
2026/10/05 09:00:07  RECONNECT  00:0b:86:11:22:02  "ExampleNet"
2026/10/05 09:00:09  DISCONNECT  00:0b:86:11:22:02  "ExampleNet"
2026/10/05 09:00:10  DEAUTH  reason 3 deauthenticated because station is leaving (could be a ClientMatch move)
IP  v4 192.0.2.143/24 gw 192.0.2.1  |  v6 2001:db8:1::1234/64 slaac (+1 temp) gw fe80::1
IP after roam  v4 kept  v6 changed 2.1 s
DHCP changed (service 102013F8-584E-46C6-8C6E-A645246CFBD7)
DHCPv6 changed (service 102013F8-584E-46C6-8C6E-A645246CFBD7)
IPv6 RA/SLAAC update
JOIN TIMING  assoc 272 ms  auth 948 ms  linkup 281 ms  ipv4 1336 ms  ipv6 1542 ms
ROAM REQUEST  target any ch 44 flags 0
```

IP events use JSON type `ip`. DHCP and DHCPv6 changes use JSON type `dhcp`. Roam decision and problematic-network airportd lines can also print as event lines.

`bssid_source` can change during a run. A first run starts in cache mode and switches to live once the Location grant lands. iairport prints `Live SSID/BSSID available.` at the switch. When the first live BSSID differs from the cached one, the scan cache was stale, so iairport corrects the current association and its summary row instead of reporting a roam:

```text
2026/10/05 09:00:12  Live SSID/BSSID available.
2026/10/05 09:00:12  BSSID is 00:0b:86:11:22:02 "AP-Lobby-02"; the scan cache said 00:0b:86:11:22:01. Corrected, not counted as a roam.
```

That line uses JSON type `bssid_correction` with `cached_bssid`, `bssid`, and `ap_name`. Rows already written to the CSV files keep the cached value with `bssid_source=cache`.

## Summary

Ctrl-C, SIGTERM, and SIGHUP stop the log stream and print a summary. The summary includes elapsed time, roams, reconnects, disconnects, distinct BSSIDs, and bytes. It also prints final IPv4 and IPv6 state. When roam history exists, it prints time, SSID, BSSID, AP name, vendor, channel, join RSSI, leave RSSI, and dwell.

## CSV files

`iairport-samples.csv` columns:

```text
ts_iso,epoch_ms,state,ssid,bssid,bssid_source,ap_name,vendor,channel,width_mhz,band,phy,security,ft,mcs,nss,gi_ns,tx_rate_mbps,rssi_dbm,noise_dbm,snr_db,cca_pct,tx_retrans,tx_fail,rx_retry,bytes_in,bytes_out,bps_in,bps_out,ipv4,ipv4_gw,ipv6,ipv6_kind,ipv6_gw,ipv6_count
```

`iairport-roams.csv` columns:

```text
ts_iso,epoch_ms,kind,ssid,old_bssid,new_bssid,old_ap_name,new_ap_name,bssid_source,old_channel,new_channel,old_rssi_dbm,new_rssi_dbm,dwell_s,v4_after_roam,v6_after_roam,v4_ready_ms,v6_ready_ms,assoc_ms,auth_ms,linkup_ms,ipv4_ms,ipv6_ms,ipv4_primary_ms,ipv6_primary_ms
```

`bssid_list.txt` keeps one BSSID per line for compatibility. If an existing CSV header differs, iairport prints a warning. It writes `<existing-filename>.<epoch>.csv`, such as `iairport-samples.csv.<epoch>.csv`.

## JSON

Every JSON line is one object. Every object has `type` and `ts`. Optional values are omitted when unknown.

Sample objects can include these keys:

```text
state,interface,ip,ssid,bssid,bssid_source,ap_name,vendor,channel,width_mhz,band,phy,security,ft,mfp,mcs,nss,tx_rate_mbps,rssi_dbm,noise_dbm,snr_db,cca_pct,bytes_in,bytes_out,bps_in,bps_out
```

Transition objects can include `old_bssid`, `new_bssid`, `old_ap_name`, `new_ap_name`, `bssid_source`, `ssid`, `old_channel`, `new_channel`, `old_rssi_dbm`, `new_rssi_dbm`, and `dwell_s`. Event objects have `message` and may add fields such as `reason`, `reason_text`, `target`, `channel`, or `flags`.

```json
{"type":"ip","ts":"2026-10-05T09:00:10.125-04:00","message":"IP  v4 192.0.2.143/24 gw 192.0.2.1  |  v6 2001:db8:1::1234/64 slaac gw fe80::1","ip":{"v4":["192.0.2.143"],"v4_prefix":[24],"v4_gw":"192.0.2.1","v6":[{"addr":"2001:db8:1::1234","prefix":64,"kind":"slaac","flags":1088}],"v6_gw":"fe80::1","primary_v4":true,"primary_v6":true}}
```
