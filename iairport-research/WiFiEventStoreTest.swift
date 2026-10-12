import Foundation
import CoreWLAN
import SystemConfiguration

func stamp() -> String {
    let formatter = ISO8601DateFormatter()
    formatter.formatOptions = [.withInternetDateTime, .withFractionalSeconds]
    return formatter.string(from: Date())
}

func log(_ message: String) {
    print("\(stamp()) \(message)")
    fflush(stdout)
}

final class EventDelegate: NSObject, CWEventDelegate {
    var linkQualityTimes: [Date] = []

    func clientConnectionInterrupted() { log("callback clientConnectionInterrupted") }
    func clientConnectionInvalidated() { log("callback clientConnectionInvalidated") }
    func powerStateDidChangeForWiFiInterface(withName interfaceName: String) { log("callback powerStateDidChange interface=\(interfaceName)") }
    func ssidDidChangeForWiFiInterface(withName interfaceName: String) { log("callback ssidDidChange interface=\(interfaceName)") }
    func bssidDidChangeForWiFiInterface(withName interfaceName: String) { log("callback bssidDidChange interface=\(interfaceName)") }
    func countryCodeDidChangeForWiFiInterface(withName interfaceName: String) { log("callback countryCodeDidChange interface=\(interfaceName)") }
    func linkDidChangeForWiFiInterface(withName interfaceName: String) { log("callback linkDidChange interface=\(interfaceName)") }
    func linkQualityDidChangeForWiFiInterface(withName interfaceName: String, rssi: Int, transmitRate: Double) {
        let now = Date()
        let delta = linkQualityTimes.last.map { String(format: "%.3fs", now.timeIntervalSince($0)) } ?? "first"
        linkQualityTimes.append(now)
        log("callback linkQualityDidChange interface=\(interfaceName) rssi=\(rssi)dBm transmitRate=\(String(format: "%.1f", transmitRate))Mbps delta=\(delta)")
    }
    func modeDidChangeForWiFiInterface(withName interfaceName: String) { log("callback modeDidChange interface=\(interfaceName)") }
    func scanCacheUpdatedForWiFiInterface(withName interfaceName: String) { log("callback scanCacheUpdated interface=\(interfaceName)") }
}

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
    let allowed: [AnyClass] = [NSDictionary.self, NSString.self, NSNumber.self, NSData.self, NSArray.self, NSDate.self]
    if let object = try NSKeyedUnarchiver.unarchivedObject(ofClasses: allowed, from: data) as? NSDictionary {
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
    throw NSError(domain: "CachedScanRecord", code: 1, userInfo: [NSLocalizedDescriptionKey: "NSKeyedUnarchiver did not return NSDictionary"])
}

func readCachedScanRecord(interface: String) {
    let key = "State:/Network/Interface/\(interface)/AirPort" as CFString
    guard let state = SCDynamicStoreCopyValue(nil, key) as? [String: Any] else {
        log("dynamicStore read failed key=\(key)")
        return
    }
    guard let data = state["CachedScanRecord"] as? Data else {
        log("dynamicStore CachedScanRecord missing key=\(key)")
        return
    }
    do {
        let record = try decodeCachedScanRecord(from: data)
        log("dynamicStore decoded \(record)")
    } catch {
        log("dynamicStore decode error=\(error)")
    }
}

let interfaceName = CWWiFiClient.shared().interface()?.interfaceName ?? "en0"
log("interface=\(interfaceName)")
readCachedScanRecord(interface: interfaceName)

let client = CWWiFiClient.shared()
let delegate = EventDelegate()
client.delegate = delegate

let events: [(String, CWEventType)] = [
    ("ssidDidChange", .ssidDidChange),
    ("bssidDidChange", .bssidDidChange),
    ("linkDidChange", .linkDidChange),
    ("linkQualityDidChange", .linkQualityDidChange),
    ("scanCacheUpdated", .scanCacheUpdated),
    ("modeDidChange", .modeDidChange),
    ("powerDidChange", .powerDidChange),
    ("countryCodeDidChange", .countryCodeDidChange),
]

for (name, event) in events {
    do {
        try client.startMonitoringEvent(with: event)
        log("startMonitoringEvent ok event=\(name)")
    } catch {
        log("startMonitoringEvent error event=\(name) error=\(error)")
    }
}

let callback: SCDynamicStoreCallBack = { _, changedKeys, _ in
    let keys = (changedKeys as NSArray as? [String]) ?? []
    log("dynamicStore callback keys=\(keys)")
}

var context = SCDynamicStoreContext(version: 0, info: nil, retain: nil, release: nil, copyDescription: nil)
var dynamicStore: SCDynamicStore?
if let store = SCDynamicStoreCreate(nil, "WiFiEventStoreTest" as CFString, callback, &context) {
    dynamicStore = store
    let keys = ["State:/Network/Interface/\(interfaceName)/AirPort" as CFString] as CFArray
    let patterns = ["State:/Network/Interface/\(interfaceName)/AirPort.*" as CFString] as CFArray
    let keysOK = SCDynamicStoreSetNotificationKeys(store, keys, patterns)
    let queueOK = SCDynamicStoreSetDispatchQueue(store, DispatchQueue.main)
    log("dynamicStore notification keysOK=\(keysOK) queueOK=\(queueOK) key=State:/Network/Interface/\(interfaceName)/AirPort pattern=State:/Network/Interface/\(interfaceName)/AirPort.*")
} else {
    log("dynamicStore create failed")
}

DispatchQueue.main.asyncAfter(deadline: .now() + 45.0) {
    log("summary linkQualityCount=\(delegate.linkQualityTimes.count)")
    if delegate.linkQualityTimes.count > 1 {
        let deltas = zip(delegate.linkQualityTimes.dropFirst(), delegate.linkQualityTimes).map { $0.timeIntervalSince($1) }
        let avg = deltas.reduce(0, +) / Double(deltas.count)
        log("summary linkQualityIntervalAvg=\(String(format: "%.3f", avg))s samples=\(deltas.map { String(format: "%.3f", $0) }.joined(separator: ","))")
    }
    exit(0)
}

RunLoop.main.run()
