# Wi-Fi roaming diagnostics CLI research report

Environment: macOS 26.5.1 build 25F80. Swift compiler: `/usr/bin/swiftc`, Apple Swift 6.3.3. All commands ran without `sudo`.

Regexes were tested with `grep -E`. Swift snippets were compiled with `/usr/bin/swiftc`.

## A. Unified-log line formats

Command shape used for historical searches:

```sh
log show --last 3d --predicate 'process == "airportd"' --info --style compact | grep -E '...'
```

The 2026-10-05 08:54 to 08:56 window had Wi-Fi debug logging enabled. Lines outside that window are treated as default debug-off evidence.

### A1. `LQM:` link quality lines

Examples:

```text
2026-10-04 16:41:34.152 Df airportd[483:e6ea3b] [com.apple.WiFiManager:] [corewifi] LQM: rssi=-59dBm per_ant_rssi=(-60dBm, -61dBm) noise=-90dBm snr=25 cca=11.0% ccaSelfTotal=6 ccaOtherTotal=3 interferenceTotal=1 txRate=576.5Mbps txFrames=416582 txFail=46 txRetrans=25709 txFallbackRate=136.1Mbps rxRate=612.5Mbps rxFrames=744616 rxRetryFrames=8084 rxToss=0 beaconRecv=0 beaconSched=0 txFwFrames=39894 txFwFail=163 txFwRetrans=1502 network = <redacted> bssid = <redacted> channel = 136 BW = 80 band = 2
2026-10-04 16:41:39.174 Df airportd[483:e6eb55] [com.apple.WiFiManager:] [corewifi] LQM: rssi=-59dBm per_ant_rssi=(-60dBm, -62dBm) noise=-90dBm snr=24 cca=11.0% ccaSelfTotal=6 ccaOtherTotal=3 interferenceTotal=1 txRate=544.4Mbps txFrames=33 txFail=0 txRetrans=22 txFallbackRate=136.1Mbps rxRate=648.5Mbps rxFrames=43 rxRetryFrames=2 rxToss=5 beaconRecv=48 beaconSched=48 txFwFrames=15 txFwFail=0 txFwRetrans=0 network = <redacted> bssid = <redacted> channel = 136 BW = 80 band = 2
2026-10-04 16:45:02.320 Df airportd[483:e6e8db] [com.apple.WiFiManager:] [corewifi] LQM: rssi=0dBm per_ant_rssi=(0dBm, 0dBm) cca=1.0% ccaSelfTotal=0 ccaOtherTotal=1 interferenceTotal=0 txRate=0.0Mbps txFrames=0 txFail=0 txRetrans=0 txFallbackRate=0.0Mbps rxRate=0.0Mbps rxFrames=0 rxRetryFrames=0 rxToss=0  network = <redacted> bssid = <redacted> channel = 136 BW = 80 band = 2
```

Fields observed in 3 days:

| Field | Unit or format in log |
| --- | --- |
| `rssi` | `dBm` suffix |
| `per_ant_rssi` | tuple of `dBm` values |
| `noise` | `dBm` suffix. Missing in the `rssi=0` sample |
| `snr` | integer, no suffix |
| `cca` | percent suffix |
| `ccaSelfTotal` | integer, no suffix |
| `ccaOtherTotal` | integer, no suffix |
| `interferenceTotal` | integer, no suffix |
| `txRate` | `Mbps` suffix |
| `txFrames` | integer counter |
| `txFail` | integer counter |
| `txRetrans` | integer counter |
| `txFallbackRate` | `Mbps` suffix |
| `rxRate` | `Mbps` suffix |
| `rxFrames` | integer counter |
| `rxRetryFrames` | integer counter |
| `rxToss` | integer counter |
| `beaconRecv` | integer counter. Missing in the `rssi=0` sample |
| `beaconSched` | integer counter. Missing in the `rssi=0` sample |
| `txFwFrames` | integer counter |
| `txFwFail` | integer counter |
| `txFwRetrans` | integer counter |
| `network` | string, redacted by unified log |
| `bssid` | string, redacted by unified log |
| `channel` | integer channel number |
| `BW` | integer, no suffix in log |
| `band` | integer, no suffix in log |

Tested regex:

```regex
LQM: rssi=-?[0-9]+dBm per_ant_rssi=\(-?[0-9]+dBm(, -?[0-9]+dBm)*\)( noise=-?[0-9]+dBm)?( snr=[0-9]+)? cca=[0-9.]+% ccaSelfTotal=[0-9]+ ccaOtherTotal=[0-9]+ interferenceTotal=[0-9]+ txRate=[0-9.]+Mbps txFrames=[0-9]+ txFail=[0-9]+ txRetrans=[0-9]+ txFallbackRate=[0-9.]+Mbps rxRate=[0-9.]+Mbps rxFrames=[0-9]+ rxRetryFrames=[0-9]+ rxToss=[0-9]+( beaconRecv=[0-9]+ beaconSched=[0-9]+)? +txFwFrames=[0-9]+ txFwFail=[0-9]+ txFwRetrans=[0-9]+ network = .* bssid = .* channel = [0-9]+ BW = [0-9]+ band = [0-9]+
```

Regex test result:

```text
lqm_count=24
```

That count was from the saved 08:54 to 08:56 sample.

### A2. Roam events

Examples visible with debug logging off and `--info`:

```text
2026-10-04 16:41:52.380 Df airportd[483:e6eb78] [com.apple.WiFiManager:] Roam: <airport[483]> +[CWRoamingProfileConfiguration roamingProfileTypeForSSID:security:scanResults:]_block_invoke: Found matching scan result with matching SSID and security on channel <CWChannel: 0x7efaf38b0> [channelNumber=136(5GHz), channelWidth={80MHz}, DFS]
2026-10-04 16:41:52.381 Df airportd[483:e6eb78] [com.apple.WiFiManager:] Roam: <airport[483]> Network : [<redacted>]. Num BSSID : [5]. 2GHz present : [0]. 5GHz present : [5]. 6GHz present : [0]. Roaming profile type : [Multi]
2026-10-04 16:41:52.405 Df airportd[483:e6e8db] [com.apple.WiFiManager:] Roam: <airport[483]> ROAMING PROFILES updated to MULTI-AP for en0, roamParamsType[3]/CWRoamParamsTypeMultiAP
2026-10-04 16:41:52.406 Df airportd[483:e6e8db] [com.apple.WiFiManager:] Roam: <airport[483]> -[CWXPCInterfaceContext __triggerRoamWithInterface:roamToBssid:roamToChannel:channelFlags:bandPreference:roamFlags:]: BEST CONNECTED ROAM triggered
2026-10-04 16:41:52.406 Df airportd[483:e6e8db] [com.apple.WiFiManager:] Roam: <airport[483]> -[CWXPCInterfaceContext __triggerRoamWithInterface:roamToBssid:roamToChannel:channelFlags:bandPreference:roamFlags:]: Requesting Roam : {
```

Search terms with zero matches in the 3-day roam set:

```text
BSS transition, BTM, 802.11v, 11k, neighbor report, candidate, Candidate, RoamReason, roamReason
```

Tested regex:

```regex
Roam: <airport\[[0-9]+\]> .*(BEST CONNECTED ROAM triggered|Requesting Roam : \{|Found (matching scan result|one matching scan result|more than three matching scan results)|Network : \[.*\]\. Num BSSID : \[[0-9]+\].*Roaming profile type : \[[^]]+\]|ROAMING PROFILES updated to [A-Z-]+ for [[:alnum:]]+)
```

Regex test result:

```text
roam_count=933
```

### A3. Deauthentication and disassociation

No verified `airportd` lines were found in 3 days for these exact formats or terms:

```text
Received Deauth
Received Disassoc
with Reason <number>
Deauth ... Reason
Disassoc ... Reason
reason code
```

Observed `disassoc=[timestamp (n)]` fields inside saved known-network objects. Those are history metadata, not live deauthentication or disassociation events.

Fallback regex, unverified against this machine's 3-day airportd logs:

```regex
Received (Deauth|Disassoc) from (<redacted>|([0-9A-Fa-f]{2}:){5}[0-9A-Fa-f]{2}) with Reason [0-9]+
```

Synthetic test line:

```text
2026-10-05 00:00:00.000 Df airportd[483:abcd] [com.apple.WiFiManager:] [corewifi] Received Deauth from d0:4d:c6:b2:78:40 with Reason 4
```

Regex test result:

```text
deauth_synthetic_count=1
```

### A4. DHCP

Examples visible with debug logging off and `--info`:

```text
2026-10-04 16:46:19.160 Df airportd[483:1185] [com.apple.WiFiManager:] SC: <airport[483]> _SC_callback: Changed keys: count[  1] key@[  0] = 'State:/Network/Service/102013F8-584E-46C6-8C6E-A645246CFBD7/DHCP'
2026-10-04 16:46:19.167 Df airportd[483:e6f040] [com.apple.WiFiManager:] SC: <airport[483]> airportdProcessSystemConfigurationEvent: Processing DHCP: 'State:/Network/Service/102013F8-584E-46C6-8C6E-A645246CFBD7/DHCP'
2026-10-04 16:46:19.173 Df airportd[483:e6f040] [com.apple.WiFiManager:] SC: <airport[483]> _processDHCPChanges: DHCP airport_changed = 1, service:102013F8-584E-46C6-8C6E-A645246CFBD7
2026-10-04 16:46:19.177 Df airportd[483:e6f040] [com.apple.WiFiManager:] SC: <airport[483]> _setDHCPMessage: dhcpInfoKey "State:/Network/Interface/en0/AirPort/DHCP Message" = (null)
```

No `IPConfigLatency`, `DHCP latency`, or `lease` lines were found in the saved DHCP match set.

Tested regex:

```regex
(CWFEventTypeDHCPChanged|Processing DHCP: 'State:/Network/Service/[0-9A-F-]+/DHCP'|_processDHCPChanges: (State:/Network/Service/[0-9A-F-]+/DHCP|DHCP airport_changed = [0-9]+, service:[0-9A-F-]+)|_setDHCPMessage: dhcpInfoKey "State:/Network/Interface/[^/]+/AirPort/DHCP Message" = |IPConfigLatency|DHCP latency|lease)
```

Regex test result:

```text
dhcp_count=15
```

### A5. Link up, link down, and association

Examples visible with debug logging off and `--info`:

```text
2026-10-04 16:56:19.375 Df airportd[483:e6f3ed] [com.apple.WiFiManager:] [corewifi] AUTO-JOIN: Auto-join triggered (uuid=75C0F, addedAt=(null), params=trigger=retry (assoc), mode=best, preferredChannels=((null)), target=((null)), autoHotspot=0, throttled=0)
2026-10-04 23:14:20.645 Df airportd[483:ef46ac] [com.apple.WiFiManager:] [corewifi] AUTO-JOIN: Updated join status (uuid=B1D76, intf=en0, ssid='<redacted>' (<redacted>), error=0, eap=[sup=2 mode=1 state=2 client=0], start=2026-10-03 19:41:01.056 -0400, assoc=2026-10-03 19:41:01.328 -0400 (272ms), auth=2026-10-03 19:41:02.004 -0400 (948ms), linkup=2026-10-03 19:41:01.338 -0400 (281ms), end=2026-10-03 19:41:01.338 -0400 (281ms), ipv4=2026-10-03 19:41:02.392 -0400 (1336ms), ipv4Primary=2026-10-03 19:41:03.886 -0400 (2829ms), ipv6=2026-10-03 19:41:02.599 -0400 (1542ms), ipv6Primary=2026-10-03 19:41:03.591 -0400 (2534ms), auto=yes, durationForJoinPMAssertion=0.000000)
2026-10-04 23:14:20.648 Df airportd[483:ef5fca] [com.apple.WiFiManager:] [corewifi] Received link change event - linkDown (0), debounce (0)
```

No literal `Link UP` or `Link DOWN` lines were found. Link-up timing appears as `linkup=` in `AUTO-JOIN: Updated join status`.

Tested regex:

```regex
(Received link change event - linkDown \([0-9]+\), debounce \([0-9]+\)|AUTO-JOIN: (Will run request|Auto-join triggered|Updated join status|Updated associated network|Auto-join aborted|Scheduling auto-join retry)|Link (UP|DOWN)|Disconnected|disconnect)
```

Regex test result:

```text
link_count=843
```

### A6. `log stream --debug` comparison

Commands:

```sh
/opt/homebrew/bin/timeout 20 log stream --predicate 'process == "airportd"' --info --style compact
/opt/homebrew/bin/timeout 20 log stream --predicate 'process == "airportd"' --info --debug --style compact
```

Results:

```text
info_rc=124 lines=365
info_debug_rc=124 lines=948
```

Both commands timed out as expected after 20 seconds.

The `--info` sample had four `LQM:` lines. The `--info --debug` sample also had four `LQM:` lines. The `--debug` sample added `Db` XPC transaction lines such as `Transaction created` and `Did find matching WiFi interface for request`.

No roam, DHCP, or link-change events occurred during the 20-second live windows. Historical `log show --info` already exposed LQM, roam profile/request, DHCP, auto-join, join status, and linkDown lines. The observed parser path should use `--info` by default. Use `--debug` only when lower-level XPC noise is useful.

## B. CoreWLAN event delivery and SystemConfiguration notifications

The Swift program compiled with:

```sh
/usr/bin/swiftc -framework CoreWLAN -framework SystemConfiguration WiFiEventStoreTest.swift -o WiFiEventStoreTest
```

Event registration output:

```text
2026-10-05T13:18:27.175Z interface=en0
2026-10-05T13:18:27.177Z dynamicStore decoded CachedScanRecord(bssid=d0:4d:c6:b2:78:40, ssid=WatchTower-Secure, channel=44, rssi=-70, noise=-95)
2026-10-05T13:18:27.177Z startMonitoringEvent ok event=ssidDidChange
2026-10-05T13:18:27.178Z startMonitoringEvent ok event=bssidDidChange
2026-10-05T13:18:27.178Z startMonitoringEvent ok event=linkDidChange
2026-10-05T13:18:27.178Z startMonitoringEvent ok event=linkQualityDidChange
2026-10-05T13:18:27.178Z startMonitoringEvent ok event=scanCacheUpdated
2026-10-05T13:18:27.179Z startMonitoringEvent ok event=modeDidChange
2026-10-05T13:18:27.179Z startMonitoringEvent ok event=powerDidChange
2026-10-05T13:18:27.179Z startMonitoringEvent ok event=countryCodeDidChange
2026-10-05T13:18:27.180Z dynamicStore notification keysOK=true queueOK=true key=State:/Network/Interface/en0/AirPort pattern=State:/Network/Interface/en0/AirPort.*
```

No `startMonitoringEvent(with:)` call threw.

Observed CoreWLAN callbacks during the 45-second run:

```text
2026-10-05T13:19:14.453Z summary linkQualityCount=0
```

No `ssidDidChange`, `bssidDidChange`, `linkDidChange`, `linkQualityDidChange`, `scanCacheUpdated`, `modeDidChange`, `powerDidChange`, or `countryCodeDidChange` delegate callback fired in this plain CLI run. No link-quality period was measurable. Unified-log `LQM:` lines were still present every 5 seconds during nearby live logging.

SystemConfiguration dynamic store notifications succeeded and did fire during normal operation:

```text
2026-10-05T13:19:07.235Z dynamicStore callback keys=["State:/Network/Interface/en0/AirPort", "State:/Network/Interface/en0/AirPort/ProfileID"]
2026-10-05T13:19:07.237Z dynamicStore callback keys=["State:/Network/Interface/en0/AirPort/SmartCCADesenseUSBPresence", "State:/Network/Interface/en0/AirPort/AutoJoinTimestamp", "State:/Network/Interface/en0/AirPort/BSSID"]
2026-10-05T13:19:07.239Z dynamicStore callback keys=["State:/Network/Interface/en0/AirPort/CachedScanRecord", "State:/Network/Interface/en0/AirPort/BluetoothAudioInUse", "State:/Network/Interface/en0/AirPort/SSID_STR"]
```

Use the exact key plus the wildcard pattern:

```swift
let keys = ["State:/Network/Interface/en0/AirPort" as CFString] as CFArray
let patterns = ["State:/Network/Interface/en0/AirPort.*" as CFString] as CFArray
SCDynamicStoreSetNotificationKeys(store, keys, patterns)
SCDynamicStoreSetDispatchQueue(store, DispatchQueue.main)
```

Retain the `SCDynamicStore`. A local variable can be released before callbacks arrive.

### CachedScanRecord decoder

This decoder uses Foundation for the archive. The dynamic store read uses SystemConfiguration.

```swift
import Foundation
import SystemConfiguration

struct CachedScanRecord: CustomStringConvertible {
    let bssid: String?
    let ssid: String?
    let channel: Int?
    let rssi: Int?
    let noise: Int?

    var description: String {
        "CachedScanRecord(bssid=\(bssid ?? "nil"), ssid=\(ssid ?? "nil"), channel=\(channel.map(String.init) ?? "nil"), rssi=\(rssi.map(String.init) ?? "nil"), noise=\(noise.map(String.init) ?? "nil"))"
    }
}

func decodeCachedScanRecord(from data: Data) throws -> CachedScanRecord {
    let allowed: [AnyClass] = [
        NSDictionary.self,
        NSString.self,
        NSNumber.self,
        NSData.self,
        NSArray.self,
        NSDate.self,
    ]

    guard let object = try NSKeyedUnarchiver.unarchivedObject(ofClasses: allowed, from: data) as? NSDictionary else {
        throw NSError(domain: "CachedScanRecord", code: 1, userInfo: [NSLocalizedDescriptionKey: "NSKeyedUnarchiver did not return NSDictionary"])
    }

    let ssidString = object["SSID_STR"] as? String
    var ssid = ssidString
    if (ssid == nil || ssid == ""), let ssidData = object["SSID"] as? Data {
        ssid = String(data: ssidData, encoding: .utf8)
    }

    return CachedScanRecord(
        bssid: object["BSSID"] as? String,
        ssid: ssid,
        channel: (object["CHANNEL"] as? NSNumber)?.intValue,
        rssi: (object["RSSI"] as? NSNumber)?.intValue,
        noise: (object["NOISE"] as? NSNumber)?.intValue
    )
}

func readCachedScanRecord(interface: String) throws -> CachedScanRecord {
    let key = "State:/Network/Interface/\(interface)/AirPort" as CFString
    guard let state = SCDynamicStoreCopyValue(nil, key) as? [String: Any] else {
        throw NSError(domain: "CachedScanRecord", code: 2, userInfo: [NSLocalizedDescriptionKey: "missing AirPort dynamic-store dictionary"])
    }
    guard let data = state["CachedScanRecord"] as? Data else {
        throw NSError(domain: "CachedScanRecord", code: 3, userInfo: [NSLocalizedDescriptionKey: "missing CachedScanRecord"])
    }
    return try decodeCachedScanRecord(from: data)
}

let record = try readCachedScanRecord(interface: "en0")
print(record)
```

Test output:

```text
CachedScanRecord(bssid=d0:4d:c6:b2:78:40, ssid=WatchTower-Secure, channel=44, rssi=-70, noise=-95)
```

## C. Interface byte counters in Swift

Tested code:

```swift
import Foundation
import Darwin

struct InterfaceCounters {
    let ibytes: UInt64
    let obytes: UInt64
}

enum CounterError: Error, CustomStringConvertible {
    case getifaddrsFailed(Int32)
    case interfaceNotFound(String)

    var description: String {
        switch self {
        case .getifaddrsFailed(let errnoValue): return "getifaddrs failed errno=\(errnoValue)"
        case .interfaceNotFound(let name): return "interface not found: \(name)"
        }
    }
}

func counters(for interfaceName: String) throws -> InterfaceCounters {
    var first: UnsafeMutablePointer<ifaddrs>?
    if getifaddrs(&first) != 0 {
        throw CounterError.getifaddrsFailed(errno)
    }
    defer { freeifaddrs(first) }

    var cursor = first
    while let current = cursor {
        let ifa = current.pointee
        if let address = ifa.ifa_addr,
           Int32(address.pointee.sa_family) == AF_LINK,
           String(cString: ifa.ifa_name) == interfaceName,
           let dataPointer = ifa.ifa_data {
            let data = dataPointer.assumingMemoryBound(to: if_data.self).pointee
            return InterfaceCounters(ibytes: UInt64(data.ifi_ibytes), obytes: UInt64(data.ifi_obytes))
        }
        cursor = ifa.ifa_next
    }
    throw CounterError.interfaceNotFound(interfaceName)
}

let interfaceName = CommandLine.arguments.dropFirst().first ?? "en0"
let first = try counters(for: interfaceName)
print("sample=1 interface=\(interfaceName) ibytes=\(first.ibytes) obytes=\(first.obytes)")
sleep(1)
let second = try counters(for: interfaceName)
print("sample=2 interface=\(interfaceName) ibytes=\(second.ibytes) obytes=\(second.obytes) deltaI=\(second.ibytes - first.ibytes) deltaO=\(second.obytes - first.obytes)")
```

Compile and run:

```sh
/usr/bin/swiftc IfCounters.swift -o IfCounters
./IfCounters en0
```

Output:

```text
sample=1 interface=en0 ibytes=2466366464 obytes=2487603200
sample=2 interface=en0 ibytes=2466446336 obytes=2487610368 deltaI=79872 deltaO=7168
```

## D. `wdutil info` timing and non-sudo behavior

`sudo` was not run. Non-sudo behavior was measured:

```sh
/usr/bin/time -p wdutil info
```

Output:

```text
rc=0
usage: sudo wdutil diagnose [-q] [-f outputDirectoryPath]
            -q may be specified to suppress legal prompt and Finder window
       sudo wdutil info
       sudo wdutil log [{+|-} {system|wifi}]+
       sudo wdutil dump
       sudo wdutil clean
       sudo wdutil privateMAC={0/1}
real 0.01
user 0.00
sys 0.00
```

Implement the privileged path behind `geteuid() == 0`. Running `wdutil info` without root only returns usage on this host.

## Implementer notes

1. Prefer the SystemConfiguration `CachedScanRecord` for SSID and BSSID in a plain CLI. It returned unredacted `bssid=d0:4d:c6:b2:78:40` and `ssid=WatchTower-Secure`.
2. Use unified log `LQM:` for periodic link quality. It appeared every 5 seconds with `--info`.
3. Do not depend on CoreWLAN delegate callbacks in a plain CLI. Registration succeeded, but no callbacks fired in the 45-second run.
4. Watch `State:/Network/Interface/en0/AirPort` and `State:/Network/Interface/en0/AirPort.*`. Retain the `SCDynamicStore` object.
5. Parse roam lines as profile, scan-result, and request signals. No 802.11v BTM, 11k, neighbor report, candidate, or roam reason lines were present in the 3-day sample.
6. Use `getifaddrs` and `if_data` for byte counters. It avoids shelling out.
7. Keep `log stream` at `--info` by default. `--debug` added XPC transaction noise in the 20-second sample.
8. Gate `wdutil info` on root. Non-root `wdutil info` returns usage in about 0.01 seconds.
