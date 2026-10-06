import Foundation
import Darwin

public final class Renderer {
    public let jsonMode: Bool
    public let colorEnabled: Bool
    public var onOutputClosed: (() -> Void)?
    public var sessionLog: SessionLog?
    private let tty: Bool
    private var lastStatus: String = ""
    private var outputClosed = false

    public init(noColor: Bool, jsonMode: Bool) {
        _ = setvbuf(stdout, nil, _IOLBF, 0)
        self.jsonMode = jsonMode
        tty = isatty(STDOUT_FILENO) == 1 && !jsonMode
        colorEnabled = tty && !noColor && !jsonMode
    }

    public func status(line: String, color: TextColor = .green) {
        guard !jsonMode else { return }
        sessionLog?.write(line)
        // A wrapped status line cannot be redrawn with \r, so clip it to the window.
        let text = tty ? Renderer.clip(line, toColumns: Renderer.terminalColumns()) : line
        let rendered = TextStyle.apply(text, color: color, enabled: colorEnabled, bold: false)
        lastStatus = rendered
        if tty {
            writeRaw("\r\u{001B}[2K\(rendered)")
        } else {
            writeLine(rendered)
        }
    }

    static func terminalColumns() -> Int? {
        var size = winsize()
        guard ioctl(STDOUT_FILENO, TIOCGWINSZ, &size) == 0, size.ws_col > 0 else { return nil }
        return Int(size.ws_col)
    }

    static func clip(_ line: String, toColumns columns: Int?) -> String {
        guard let columns, columns > 1, line.count >= columns else { return line }
        return String(line.prefix(columns - 1))
    }

    public func event(line: String, color: TextColor = .none, bold: Bool = false, redrawStatus: Bool = true) {
        guard !jsonMode else { return }
        sessionLog?.write(line)
        let rendered = TextStyle.apply(line, color: color, enabled: colorEnabled, bold: bold)
        if tty {
            writeRaw("\r\u{001B}[2K\(rendered)\n")
            if redrawStatus && !lastStatus.isEmpty {
                writeRaw("\r\(lastStatus)")
            }
        } else {
            writeLine(rendered)
        }
    }

    public func json(_ object: [String: Any]) {
        let line = JSONLine.encode(object)
        sessionLog?.write(line)
        writeLine(line)
    }

    public func finishLine() {
        if tty { writeRaw("\n") }
    }

    public func clearStatus() {
        lastStatus = ""
        if tty { writeRaw("\r\u{001B}[2K") }
    }

    @discardableResult
    private func writeLine(_ value: String) -> Bool {
        writeRaw(value + "\n")
    }

    @discardableResult
    private func writeRaw(_ value: String) -> Bool {
        guard !outputClosed else { return false }
        let bytes = Array(value.utf8)
        let result = bytes.withUnsafeBytes { buffer in
            Darwin.write(STDOUT_FILENO, buffer.baseAddress, buffer.count)
        }
        if result < 0 && errno == EPIPE {
            outputClosed = true
            onOutputClosed?()
            return false
        }
        fflush(stdout)
        return result >= 0
    }
}

public enum OutputFormatter {
    public static func status(sample: LinkSample, verbose: Bool, time: TimeFormatter) -> (String, TextColor) {
        let ts = time.status(sample.timestamp)
        switch sample.status {
        case .off:
            return ("\(ts)  Wi-Fi is off", .red)
        case .disconnected:
            return ("\(ts)  Not connected", .yellow)
        case .associated:
            let ssid = sample.ssid ?? ""
            let vendorPrefix = sample.vendor.map { "\($0) " } ?? ""
            let bssid = bssidDisplay(sample.bssid, source: sample.bssidSource)
            let apName = sample.apName.map { " \($0)" } ?? ""
            var parts: [String] = []
            var channel = "Chan "
            if let ch = sample.channel { channel += "\(ch)" } else { channel += "?" }
            if let width = sample.widthMHz { channel += "/\(width)" }
            parts.append(channel)
            if let phy = sample.phy { parts.append(phy) }
            if let security = sample.security {
                var sec = security
                if sample.ft { sec += " FT" }
                if sample.mfp { sec += " MFP" }
                parts.append(sec)
            }
            if let mcs = sample.mcs { parts.append("MCS \(mcs)") }
            if let nss = sample.nss { parts.append("NSS \(nss)") }
            if let rate = sample.txRateMbps, rate > 0 { parts.append(String(format: "(%.0f Mbps)", rate)) }
            var quality: [String] = []
            if let rssi = sample.rssiDBM { quality.append("RSSI \(rssi)") }
            if let noise = sample.noiseDBM { quality.append("NF \(noise)") }
            if let snr = sample.snrDB { quality.append("SNR \(snr)") }
            if let cca = sample.ccaPct { quality.append("CCA \(cca)%") }
            var suffix: [String] = []
            let bps = (sample.bpsIn ?? 0) + (sample.bpsOut ?? 0)
            let bytes = (sample.bytesIn ?? 0) + (sample.bytesOut ?? 0)
            suffix.append("Speed: \(Units.rate(bps))bps")
            suffix.append("Data: \(Units.data(bytes))")
            if verbose {
                if !sample.perAntennaRSSI.isEmpty {
                    suffix.append("ant(\(sample.perAntennaRSSI.map(String.init).joined(separator: ",")))")
                }
                if let txRetrans = sample.txRetrans { suffix.append("rtx \(txRetrans)") }
                if let txFail = sample.txFail { suffix.append("fail \(txFail)") }
                suffix.append(sample.ipState.compactTag())
            }
            let line = "\(ts)  \"\(ssid)\" (\(vendorPrefix)\(bssid)\(apName)) \(parts.joined(separator: " "))  \(quality.joined(separator: " "))  \(suffix.joined(separator: " "))"
            return (line, .green)
        }
    }

    public static func transition(_ transition: AssociationTransition, time: TimeFormatter) -> (String, TextColor) {
        let ts = time.status(transition.at)
        switch transition.kind {
        case .join:
            let new = transition.new
            return ("\(ts)  JOIN  \(apDisplay(new))  \"\(new?.ssid ?? "")\"  ch \(new?.channel.map(String.init) ?? "")  RSSI \(new?.rssi.map(String.init) ?? "")", .cyan)
        case .roam:
            let old = transition.old
            let new = transition.new
            return ("\(ts)  ROAM  \(apDisplay(old)) -> \(apDisplay(new))  \"\(new?.ssid ?? old?.ssid ?? "")\"  ch \(old?.channel.map(String.init) ?? "") -> \(new?.channel.map(String.init) ?? "")  RSSI \(old?.rssi.map(String.init) ?? "") -> \(new?.rssi.map(String.init) ?? "")", .magenta)
        case .reconnect:
            let new = transition.new
            return ("\(ts)  RECONNECT  \(apDisplay(new))  \"\(new?.ssid ?? "")\"", .cyan)
        case .disconnect:
            let old = transition.old
            return ("\(ts)  DISCONNECT  \(apDisplay(old))  \"\(old?.ssid ?? "")\"", .yellow)
        }
    }

    /// Notification text for a roam: AP names when the beacons carry them, BSSIDs otherwise.
    public static func roamNotification(_ transition: AssociationTransition) -> String {
        let old = transition.old
        let new = transition.new
        var text = "\(old?.apName ?? old?.bssid ?? "") -> \(new?.apName ?? new?.bssid ?? "")"
        if let oldChannel = old?.channel, let newChannel = new?.channel {
            text += "  ch \(oldChannel) -> \(newChannel)"
        }
        return text
    }

    public static func sampleJSON(sample: LinkSample, time: TimeFormatter) -> [String: Any] {
        var object: [String: Any] = [
            "type": "sample",
            "ts": time.json(sample.timestamp),
            "state": sample.status.csvValue,
            "interface": sample.interfaceName,
            "ip": sample.ipState.jsonObject()
        ]
        set(&object, "ssid", sample.ssid)
        set(&object, "bssid", sample.bssid)
        object["bssid_source"] = sample.bssidSource.rawValue
        set(&object, "vendor", sample.vendor)
        set(&object, "ap_name", sample.apName)
        set(&object, "channel", sample.channel)
        set(&object, "width_mhz", sample.widthMHz)
        set(&object, "band", sample.band)
        set(&object, "phy", sample.phy)
        set(&object, "security", sample.security)
        object["ft"] = sample.ft
        object["mfp"] = sample.mfp
        set(&object, "mcs", sample.mcs)
        set(&object, "nss", sample.nss)
        set(&object, "tx_rate_mbps", sample.txRateMbps)
        set(&object, "rssi_dbm", sample.rssiDBM)
        set(&object, "noise_dbm", sample.noiseDBM)
        set(&object, "snr_db", sample.snrDB)
        set(&object, "cca_pct", sample.ccaPct)
        set(&object, "bytes_in", sample.bytesIn)
        set(&object, "bytes_out", sample.bytesOut)
        set(&object, "bps_in", sample.bpsIn)
        set(&object, "bps_out", sample.bpsOut)
        return object
    }

    public static func transitionJSON(_ transition: AssociationTransition, time: TimeFormatter) -> [String: Any] {
        var object: [String: Any] = [
            "type": transition.kind.rawValue,
            "ts": time.json(transition.at)
        ]
        set(&object, "old_bssid", transition.old?.bssid)
        set(&object, "new_bssid", transition.new?.bssid)
        set(&object, "old_ap_name", transition.old?.apName)
        set(&object, "new_ap_name", transition.new?.apName)
        object["bssid_source"] = (transition.new?.bssidSource ?? transition.old?.bssidSource ?? .live).rawValue
        set(&object, "ssid", transition.new?.ssid ?? transition.old?.ssid)
        set(&object, "old_channel", transition.old?.channel)
        set(&object, "new_channel", transition.new?.channel)
        set(&object, "old_rssi_dbm", transition.old?.rssi)
        set(&object, "new_rssi_dbm", transition.new?.rssi)
        set(&object, "dwell_s", transition.dwell)
        return object
    }

    public static func eventJSON(type: String, message: String, date: Date, time: TimeFormatter, extra: [String: Any] = [:]) -> [String: Any] {
        var object: [String: Any] = ["type": type, "ts": time.json(date), "message": message]
        for (key, value) in extra { object[key] = value }
        return object
    }

    private static func set(_ object: inout [String: Any], _ key: String, _ value: Any?) {
        if let value { object[key] = value }
    }

    private static func bssidDisplay(_ bssid: String?, source: BSSIDSource) -> String {
        guard let bssid else { return "" }
        if bssid == "?" { return "?" }
        return source == .cache ? "\(bssid)~" : bssid
    }

    private static func apDisplay(_ info: AssociationInfo?) -> String {
        let bssid = bssidDisplay(info?.bssid, source: info?.bssidSource ?? .live)
        guard let name = info?.apName else { return bssid }
        return "\(bssid) (\(name))"
    }
}

public struct RoamHistoryEntry: Equatable {
    public var number: Int
    public var joinTime: Date
    public var ssid: String?
    public var bssid: String
    public var apName: String?
    public var vendor: String?
    public var channel: Int?
    public var rssiAtJoin: Int?
    public var rssiAtLeave: Int?
    public var leaveTime: Date?

    public init(number: Int, joinTime: Date, ssid: String?, bssid: String, apName: String? = nil, vendor: String?, channel: Int?, rssiAtJoin: Int?) {
        self.number = number
        self.joinTime = joinTime
        self.ssid = ssid
        self.bssid = bssid
        self.apName = apName
        self.vendor = vendor
        self.channel = channel
        self.rssiAtJoin = rssiAtJoin
    }

    public static let header = "#  time  ssid  bssid  ap_name  vendor  ch  rssi_at_join  rssi_at_leave  dwell"

    public func line(time: TimeFormatter, now: Date) -> String {
        let leave = leaveTime ?? now
        let dwell = Units.elapsed(leave.timeIntervalSince(joinTime))
        return String(format: "%2d  %@  %@  %@  %@  %@  %@  %@  %@  %@",
                      number,
                      time.status(joinTime),
                      ssid ?? "",
                      bssid,
                      apName ?? "",
                      vendor ?? "",
                      channel.map(String.init) ?? "",
                      rssiAtJoin.map(String.init) ?? "",
                      rssiAtLeave.map(String.init) ?? "",
                      dwell)
    }
}

public struct PendingRoamCSVRow: Equatable {
    public var transition: AssociationTransition
    public var timing: JoinTiming?

    public init(transition: AssociationTransition, timing: JoinTiming?) {
        self.transition = transition
        self.timing = timing
    }
}

public struct PendingRoamCSVBuffer: Equatable {
    private var row: PendingRoamCSVRow?

    public init() {}

    public mutating func replace(transition: AssociationTransition, timing: JoinTiming?) -> PendingRoamCSVRow? {
        let old = row
        row = PendingRoamCSVRow(transition: transition, timing: timing)
        return old
    }

    public mutating func take() -> PendingRoamCSVRow? {
        defer { row = nil }
        return row
    }
}

public final class CSVLogger {
    private let time: TimeFormatter
    private let samples: FileHandle?
    private let roams: FileHandle?
    private let bssids: FileHandle?

    public static let sampleHeader = "ts_iso,epoch_ms,state,ssid,bssid,bssid_source,ap_name,vendor,channel,width_mhz,band,phy,security,ft,mcs,nss,gi_ns,tx_rate_mbps,rssi_dbm,noise_dbm,snr_db,cca_pct,tx_retrans,tx_fail,rx_retry,bytes_in,bytes_out,bps_in,bps_out,ipv4,ipv4_gw,ipv6,ipv6_kind,ipv6_gw,ipv6_count"
    public static let roamHeader = "ts_iso,epoch_ms,kind,ssid,old_bssid,new_bssid,old_ap_name,new_ap_name,bssid_source,old_channel,new_channel,old_rssi_dbm,new_rssi_dbm,dwell_s,v4_after_roam,v6_after_roam,v4_ready_ms,v6_ready_ms,assoc_ms,auth_ms,linkup_ms,ipv4_ms,ipv6_ms,ipv4_primary_ms,ipv6_primary_ms"

    public init?(enabled: Bool, time: TimeFormatter, warnings: inout [String]) {
        guard enabled else { return nil }
        self.time = time
        samples = CSVLogger.open(path: "iairport-samples.csv", header: Self.sampleHeader, warnings: &warnings)
        roams = CSVLogger.open(path: "iairport-roams.csv", header: Self.roamHeader, warnings: &warnings)
        bssids = CSVLogger.openText(path: "bssid_list.txt")
    }

    public func writeSample(_ sample: LinkSample) {
        guard let samples else { return }
        let v6 = sample.ipState.displayIPv6
        let row = CSV.row([
            time.csv(sample.timestamp), epochMS(sample.timestamp), sample.status.csvValue, sample.ssid, sample.bssid, sample.bssidSource.rawValue, sample.apName, sample.vendor,
            sample.channel.map(String.init), sample.widthMHz.map(String.init), sample.band, sample.phy, sample.security,
            sample.ft ? "1" : "0", sample.mcs.map(String.init), sample.nss.map(String.init), sample.guardIntervalNS.map(String.init), sample.txRateMbps.map { String(format: "%.1f", $0) },
            sample.rssiDBM.map(String.init), sample.noiseDBM.map(String.init), sample.snrDB.map(String.init), sample.ccaPct.map(String.init), sample.txRetrans.map(String.init), sample.txFail.map(String.init), sample.rxRetry.map(String.init),
            sample.bytesIn.map(String.init), sample.bytesOut.map(String.init), sample.bpsIn.map(String.init), sample.bpsOut.map(String.init),
            sample.ipState.ipv4.joined(separator: " "), sample.ipState.ipv4Router, v6?.address, v6?.kind.rawValue, sample.ipState.ipv6Router, String(sample.ipState.ipv6.count)
        ])
        samples.write(Data(row.utf8))
    }

    public func writeTransition(_ transition: AssociationTransition, timing: JoinTiming?, v4: IPAfterRoam? = nil, v6: IPAfterRoam? = nil) {
        guard let roams else { return }
        let row = CSV.row([
            time.csv(transition.at), epochMS(transition.at), transition.kind.rawValue, transition.new?.ssid ?? transition.old?.ssid,
            transition.old?.bssid, transition.new?.bssid, transition.old?.apName, transition.new?.apName, (transition.new?.bssidSource ?? transition.old?.bssidSource)?.rawValue, transition.old?.channel.map(String.init), transition.new?.channel.map(String.init),
            transition.old?.rssi.map(String.init), transition.new?.rssi.map(String.init), transition.dwell.map { String(format: "%.1f", $0) },
            v4?.csvValue, v6?.csvValue, v4?.milliseconds.map(String.init), v6?.milliseconds.map(String.init),
            timing?.values["assoc"].map(String.init), timing?.values["auth"].map(String.init), timing?.values["linkup"].map(String.init),
            timing?.values["ipv4"].map(String.init), timing?.values["ipv6"].map(String.init), timing?.values["ipv4Primary"].map(String.init), timing?.values["ipv6Primary"].map(String.init)
        ])
        roams.write(Data(row.utf8))
    }

    public func writeBSSID(_ bssid: String?) {
        guard let bssids, let bssid else { return }
        bssids.write(Data("\(bssid)\n".utf8))
    }

    public func close() {
        try? samples?.close()
        try? roams?.close()
        try? bssids?.close()
    }

    private static func open(path: String, header: String, warnings: inout [String]) -> FileHandle? {
        var finalPath = path
        if FileManager.default.fileExists(atPath: path), let first = firstLine(path: path), !first.isEmpty, first != header {
            let epoch = Int(Date().timeIntervalSince1970)
            finalPath = "\(path).\(epoch).csv"
            warnings.append("warning: \(path) header mismatch, writing \(finalPath)")
        }
        if !FileManager.default.fileExists(atPath: finalPath) {
            FileManager.default.createFile(atPath: finalPath, contents: Data("\(header)\n".utf8))
        } else if fileSize(path: finalPath) == 0 {
            try? Data("\(header)\n".utf8).write(to: URL(fileURLWithPath: finalPath))
        }
        guard let handle = FileHandle(forWritingAtPath: finalPath) else { return nil }
        _ = try? handle.seekToEnd()
        return handle
    }

    private static func openText(path: String) -> FileHandle? {
        if !FileManager.default.fileExists(atPath: path) {
            FileManager.default.createFile(atPath: path, contents: nil)
        }
        guard let handle = FileHandle(forWritingAtPath: path) else { return nil }
        _ = try? handle.seekToEnd()
        return handle
    }

    private static func firstLine(path: String) -> String? {
        guard let handle = FileHandle(forReadingAtPath: path) else { return nil }
        defer { try? handle.close() }
        let data = handle.readData(ofLength: 4096)
        guard let text = String(data: data, encoding: .utf8) else { return nil }
        return text.split(separator: "\n", omittingEmptySubsequences: false).first.map(String.init)
    }

    private static func fileSize(path: String) -> UInt64 {
        let value = try? FileManager.default.attributesOfItem(atPath: path)[.size] as? NSNumber
        return value?.uint64Value ?? 0
    }

    private func epochMS(_ date: Date) -> String {
        String(Int64((date.timeIntervalSince1970 * 1000.0).rounded()))
    }
}
