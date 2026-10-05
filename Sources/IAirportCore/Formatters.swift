import Foundation

public enum TextColor {
    case green
    case red
    case cyan
    case magenta
    case yellow
    case white
    case dim
    case none
}

public enum TextStyle {
    public static func apply(_ text: String, color: TextColor, enabled: Bool, bold: Bool = false) -> String {
        guard enabled else { return text }
        var codes: [String] = []
        if bold { codes.append("1") }
        switch color {
        case .green: codes.append("32")
        case .red: codes.append("31")
        case .cyan: codes.append("36")
        case .magenta: codes.append("35")
        case .yellow: codes.append("33")
        case .white: codes.append("37")
        case .dim: codes.append("2")
        case .none: break
        }
        guard !codes.isEmpty else { return text }
        return "\u{001B}[\(codes.joined(separator: ";"))m\(text)\u{001B}[0m"
    }
}

public final class TimeFormatter {
    private let statusFormatter: DateFormatter
    private let csvFormatter: DateFormatter

    public init() {
        statusFormatter = DateFormatter()
        statusFormatter.locale = Locale(identifier: "en_US_POSIX")
        statusFormatter.dateFormat = "yyyy/MM/dd HH:mm:ss"
        csvFormatter = DateFormatter()
        csvFormatter.locale = Locale(identifier: "en_US_POSIX")
        csvFormatter.dateFormat = "yyyy-MM-dd'T'HH:mm:ss.SSSZZZZZ"
    }

    public func status(_ date: Date) -> String {
        statusFormatter.string(from: date)
    }

    public func csv(_ date: Date) -> String {
        csvFormatter.string(from: date)
    }

    public func json(_ date: Date) -> String {
        csvFormatter.string(from: date)
    }
}

public enum CSV {
    public static func field(_ value: String?) -> String {
        guard let value else { return "" }
        if value.contains(",") || value.contains("\"") || value.contains("\n") || value.contains("\r") {
            return "\"\(value.replacingOccurrences(of: "\"", with: "\"\""))\""
        }
        return value
    }

    public static func row(_ fields: [String?]) -> String {
        fields.map { field($0) }.joined(separator: ",") + "\n"
    }
}

public enum Units {
    public static func data(_ bytes: UInt64) -> String {
        if bytes == 0 { return "0" }
        if bytes < 999 { return "\(bytes)B" }
        if bytes < 999_999 { return String(format: "%.0fK", Double(bytes) / 1024.0) }
        if bytes < 999_999_999 { return String(format: "%.1fM", Double(bytes) / 1024.0 / 1024.0) }
        return String(format: "%.1fG", Double(bytes) / 1024.0 / 1024.0 / 1024.0)
    }

    public static func rate(_ bitsPerSecond: UInt64) -> String {
        if bitsPerSecond == 0 { return "0" }
        if bitsPerSecond < 999 { return "\(bitsPerSecond)" }
        if bitsPerSecond < 999_999 { return String(format: "%.0fK", Double(bitsPerSecond) / 1000.0) }
        if bitsPerSecond < 999_999_999 { return String(format: "%.1fM", Double(bitsPerSecond) / 1_000_000.0) }
        return String(format: "%.1fG", Double(bitsPerSecond) / 1_000_000_000.0)
    }

    public static func elapsed(_ seconds: TimeInterval) -> String {
        let total = Int(seconds.rounded())
        let h = total / 3600
        let m = (total % 3600) / 60
        let s = total % 60
        if h > 0 { return String(format: "%dh %02dm %02ds", h, m, s) }
        if m > 0 { return String(format: "%dm %02ds", m, s) }
        return "\(s)s"
    }
}

public enum MACAddress {
    private static let validPattern = try! NSRegularExpression(pattern: "^([0-9a-f]{2}:){5}[0-9a-f]{2}$")
    private static let invalidValues: Set<String> = [
        "00:00:00:00:00:00",
        "02:00:00:00:00:00",
        "ff:ff:ff:ff:ff:ff"
    ]

    public static func normalize(_ value: String?) -> String? {
        guard var value else { return nil }
        value = value.trimmingCharacters(in: .whitespacesAndNewlines).lowercased()
        value = value.replacingOccurrences(of: "-", with: ":")
        if value.contains(".") {
            let compact = value.replacingOccurrences(of: ".", with: "")
            if compact.count == 12 {
                value = stride(from: 0, to: 12, by: 2).map { index in
                    let start = compact.index(compact.startIndex, offsetBy: index)
                    let end = compact.index(start, offsetBy: 2)
                    return String(compact[start..<end])
                }.joined(separator: ":")
            }
        }
        // CachedScanRecord and other ether_ntoa-style sources drop leading
        // zeros, so "68:51:34:7c:32:1" means "68:51:34:7c:32:01".
        let octets = value.split(separator: ":", omittingEmptySubsequences: false)
        if octets.count == 6, octets.allSatisfy({ (1...2).contains($0.count) }) {
            value = octets.map { $0.count == 1 ? "0" + $0 : String($0) }.joined(separator: ":")
        }
        let range = NSRange(value.startIndex..<value.endIndex, in: value)
        guard validPattern.firstMatch(in: value, range: range) != nil else { return nil }
        guard !invalidValues.contains(value) else { return nil }
        return value
    }

    public static func oui(_ bssid: String?) -> String? {
        guard let bssid = normalize(bssid) else { return nil }
        return bssid.split(separator: ":").prefix(3).joined(separator: ":").uppercased()
    }
}

public enum JSONLine {
    public static func encode(_ object: [String: Any]) -> String {
        guard JSONSerialization.isValidJSONObject(object),
              let data = try? JSONSerialization.data(withJSONObject: object, options: [.sortedKeys]),
              let text = String(data: data, encoding: .utf8) else {
            return "{\"type\":\"log\",\"message\":\"json encoding failed\"}"
        }
        return text
    }
}
