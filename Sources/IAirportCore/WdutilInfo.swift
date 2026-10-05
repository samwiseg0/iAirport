import Foundation
import Darwin

public struct WdutilMetrics: Equatable {
    public var rssi: Int?
    public var noise: Int?
    public var txRate: Double?
    public var phy: String?
    public var mcs: Int?
    public var nss: Int?
    public var guardIntervalNS: Int?
    public var channel: String?
    public var cca: Int?
    public var security: String?
    public var ssid: String?
    public var bssid: String?

    public init() {}
}

public enum WdutilInfo {
    public enum Privilege: Equatable {
        case direct
        case sudo
        case helper
        case unavailable

        public var description: String {
            switch self {
            case .direct: return "wdutil fields: running as root"
            case .sudo: return "wdutil fields: sudo -n available"
            case .helper: return "wdutil fields: root helper from sudo iairport"
            case .unavailable:
                if let plugin = SudoConfig.thirdPartyPlugins().first {
                    return "wdutil fields: unavailable (sudo uses the \(plugin) plugin, which ignores sudo -n)"
                }
                return "wdutil fields: unavailable (run sudo iairport or sudo -v first)"
            }
        }
    }

    public static func parseInfo(_ text: String) -> WdutilMetrics {
        var metrics = WdutilMetrics()
        for line in text.split(separator: "\n") {
            guard let separator = line.firstIndex(of: ":") else { continue }
            let key = line[..<separator].trimmingCharacters(in: .whitespaces)
            let value = line[line.index(after: separator)...].trimmingCharacters(in: .whitespaces)
            switch key {
            case "RSSI": metrics.rssi = firstInt(String(value))
            case "Noise": metrics.noise = firstInt(String(value))
            case "Tx Rate": metrics.txRate = firstDouble(String(value))
            case "PHY Mode": metrics.phy = String(value)
            case "MCS Index": metrics.mcs = firstInt(String(value))
            case "Guard Interval": metrics.guardIntervalNS = firstInt(String(value))
            case "NSS": metrics.nss = firstInt(String(value))
            case "Channel": metrics.channel = String(value)
            case "CCA": metrics.cca = firstInt(String(value))
            case "Security": metrics.security = String(value)
            case "SSID": metrics.ssid = String(value)
            case "BSSID": metrics.bssid = MACAddress.normalize(String(value))
            default: break
            }
        }
        return metrics
    }

    public static func runInfo(privilege: Privilege, completion: @escaping (WdutilMetrics?, Bool) -> Void) {
        guard privilege != .unavailable else {
            completion(nil, false)
            return
        }
        DispatchQueue.global(qos: .utility).async {
            let result = run(arguments: ["info"], privilege: privilege)
            completion(result.status == 0 ? result.text.map { parseInfo($0) } : nil, result.status == 0)
        }
    }

    public static func toggleDebug() -> Int32 {
        let privilege = effectivePrivilege()
        guard privilege != .unavailable else {
            print("Run: sudo iairport -d")
            return 1
        }
        let before = readDebugState()
        let turnOn = before != "On"
        _ = run(arguments: ["log", turnOn ? "+wifi" : "-wifi"], privilege: privilege)
        let after = readDebugState() ?? "unknown"
        print("Wi-Fi debug logging: \(after)")
        return 0
    }

    public static func readDebugState() -> String? {
        let privilege = effectivePrivilege()
        guard privilege != .unavailable, let text = run(arguments: ["log"], privilege: privilege).text else { return nil }
        for line in text.split(separator: "\n") {
            if line.range(of: "^\\s*Wi-Fi\\s*:\\s*(On|Off)", options: .regularExpression) != nil {
                return line.contains("On") ? "On" : "Off"
            }
        }
        return "unknown"
    }

    public static func effectivePrivilege() -> Privilege {
        if geteuid() == 0 { return .direct }
        if RootHelperClient.shared != nil { return .helper }
        return sudoAvailable() ? .sudo : .unavailable
    }

    /// Seconds to wait for `sudo -n true`. A plain sudoers policy answers at once.
    static let sudoProbeTimeout: TimeInterval = 3
    /// Seconds to wait for one `sudo -n wdutil` call.
    static let sudoWdutilTimeout: TimeInterval = 15

    public static func sudoAvailable() -> Bool {
        // Third-party sudo policy plugins (for example BeyondTrust/Avecto
        // Defendpoint) ignore `-n` and prompt on the terminal. Never probe them.
        guard SudoConfig.usesStandardPolicy() else { return false }
        let result = BoundedProcess.run(executable: "/usr/bin/sudo", arguments: ["-n", "true"], timeout: sudoProbeTimeout)
        return !result.timedOut && result.status == 0
    }

    /// Kills any `sudo` or `wdutil` child still running. Call before exit.
    public static func terminateChildren() {
        BoundedProcess.terminateAll()
    }

    private static func run(arguments: [String], privilege: Privilege) -> (text: String?, status: Int32) {
        let result: BoundedProcess.Result
        switch privilege {
        case .direct:
            result = BoundedProcess.run(executable: "/usr/bin/wdutil", arguments: arguments, timeout: nil)
        case .sudo:
            result = BoundedProcess.run(executable: "/usr/bin/sudo", arguments: ["-n", "/usr/bin/wdutil"] + arguments, timeout: sudoWdutilTimeout)
        case .helper:
            return RootHelperClient.shared?.run(arguments: arguments) ?? (nil, 1)
        case .unavailable:
            return (nil, 1)
        }
        if result.timedOut { return (nil, 1) }
        return (result.output.flatMap { String(data: $0, encoding: .utf8) }, result.status)
    }

    private static func firstInt(_ text: String) -> Int? {
        guard let match = text.range(of: "-?\\d+", options: .regularExpression) else { return nil }
        return Int(text[match])
    }

    private static func firstDouble(_ text: String) -> Double? {
        guard let match = text.range(of: "-?\\d+(?:\\.\\d+)?", options: .regularExpression) else { return nil }
        return Double(text[match])
    }
}

/// Reads `/etc/sudo.conf` to find sudo plugins that are not the stock sudoers ones.
public enum SudoConfig {
    public static let defaultPath = "/etc/sudo.conf"

    /// Plugin symbol names from `Plugin` lines that are not `sudoers_*`.
    public static func thirdPartyPlugins(confText: String) -> [String] {
        var names: [String] = []
        for rawLine in confText.split(whereSeparator: \.isNewline) {
            let line = rawLine.trimmingCharacters(in: .whitespaces)
            guard !line.isEmpty, !line.hasPrefix("#") else { continue }
            let fields = line.split(whereSeparator: { $0 == " " || $0 == "\t" })
            guard fields.count >= 3, fields[0] == "Plugin" else { continue }
            let name = String(fields[1])
            if !name.hasPrefix("sudoers_") { names.append(name) }
        }
        return names
    }

    public static func thirdPartyPlugins(path: String = defaultPath) -> [String] {
        guard let text = try? String(contentsOfFile: path, encoding: .utf8) else { return [] }
        return thirdPartyPlugins(confText: text)
    }

    /// True when sudo runs only the stock sudoers plugins, so `sudo -n` is honored.
    public static func usesStandardPolicy(path: String = defaultPath) -> Bool {
        thirdPartyPlugins(path: path).isEmpty
    }
}

/// Runs a child with stdin from /dev/null and an optional deadline. On timeout
/// the child is killed and the terminal settings are restored, since a sudo
/// prompt may have turned off echo.
enum BoundedProcess {
    struct Result {
        var output: Data?
        var status: Int32
        var timedOut: Bool
    }

    private final class Entry {
        let process: Process
        let terminal: termios?
        init(process: Process, terminal: termios?) {
            self.process = process
            self.terminal = terminal
        }
    }

    private final class OutputBox {
        var data = Data()
    }

    private static let lock = NSLock()
    private static var running: [ObjectIdentifier: Entry] = [:]

    static func run(executable: String, arguments: [String], timeout: TimeInterval?) -> Result {
        let process = Process()
        process.executableURL = URL(fileURLWithPath: executable)
        process.arguments = arguments
        process.standardInput = FileHandle.nullDevice
        let pipe = Pipe()
        process.standardOutput = pipe
        process.standardError = pipe
        let exited = DispatchSemaphore(value: 0)
        process.terminationHandler = { _ in exited.signal() }
        let entry = Entry(process: process, terminal: saveTerminal())
        do {
            try process.run()
        } catch {
            return Result(output: nil, status: 1, timedOut: false)
        }
        register(entry)
        defer { unregister(entry) }

        let box = OutputBox()
        let readDone = DispatchSemaphore(value: 0)
        let reader = pipe.fileHandleForReading
        DispatchQueue.global(qos: .utility).async {
            box.data = reader.readDataToEndOfFile()
            readDone.signal()
        }

        let deadline: DispatchTime = timeout.map { .now() + $0 } ?? .distantFuture
        if exited.wait(timeout: deadline) == .timedOut {
            stop(entry)
            return Result(output: nil, status: -1, timedOut: true)
        }
        // A grandchild could hold the pipe open; do not wait on it forever.
        guard readDone.wait(timeout: .now() + 2) == .success else {
            return Result(output: nil, status: process.terminationStatus, timedOut: false)
        }
        return Result(output: box.data, status: process.terminationStatus, timedOut: false)
    }

    static func terminateAll() {
        lock.lock()
        let entries = Array(running.values)
        lock.unlock()
        for entry in entries { stop(entry) }
    }

    private static func stop(_ entry: Entry) {
        let process = entry.process
        if process.isRunning {
            let pid = process.processIdentifier
            kill(pid, SIGTERM)
            let limit = Date().addingTimeInterval(0.5)
            while process.isRunning && Date() < limit {
                usleep(20_000)
            }
            if process.isRunning { kill(pid, SIGKILL) }
        }
        restoreTerminal(entry.terminal)
    }

    private static func register(_ entry: Entry) {
        lock.lock()
        running[ObjectIdentifier(entry)] = entry
        lock.unlock()
    }

    private static func unregister(_ entry: Entry) {
        lock.lock()
        running.removeValue(forKey: ObjectIdentifier(entry))
        lock.unlock()
    }

    private static func saveTerminal() -> termios? {
        guard isatty(STDIN_FILENO) == 1 else { return nil }
        var state = termios()
        return tcgetattr(STDIN_FILENO, &state) == 0 ? state : nil
    }

    private static func restoreTerminal(_ state: termios?) {
        guard var state else { return }
        _ = tcsetattr(STDIN_FILENO, TCSANOW, &state)
    }
}
