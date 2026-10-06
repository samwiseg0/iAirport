import Foundation
import Darwin

public struct SessionLogError: Error, Equatable {
    public let reason: String
}

public final class SessionLog {
    public let path: String

    private let fd: Int32
    private let lock = NSLock()
    private var closed = false

    private init(path: String, fd: Int32) {
        self.path = path
        self.fd = fd
    }

    deinit {
        close()
    }

    public static func defaultDirectory() -> String {
        NSHomeDirectory() + "/Library/Logs/iairport"
    }

    public static func fileName(for date: Date, pid: Int32? = nil) -> String {
        let formatter = DateFormatter()
        formatter.locale = Locale(identifier: "en_US_POSIX")
        formatter.timeZone = .current
        formatter.dateFormat = "yyyyMMdd-HHmmss"
        let stamp = formatter.string(from: date)
        if let pid {
            return "iairport-\(stamp)-\(pid).log"
        }
        return "iairport-\(stamp).log"
    }

    public static func open(directory: String = defaultDirectory(), now: Date = Date(), arguments: [String]) -> Result<SessionLog, SessionLogError> {
        do {
            try FileManager.default.createDirectory(atPath: directory, withIntermediateDirectories: true)
        } catch {
            return .failure(SessionLogError(reason: posixReason(from: error)))
        }

        let basePath = (directory as NSString).appendingPathComponent(fileName(for: now))
        var finalPath = basePath
        var fd = Darwin.open(finalPath, O_WRONLY | O_CREAT | O_EXCL | O_APPEND, S_IRUSR | S_IWUSR | S_IRGRP | S_IROTH)
        if fd < 0 && errno == EEXIST {
            finalPath = (directory as NSString).appendingPathComponent(fileName(for: now, pid: getpid()))
            fd = Darwin.open(finalPath, O_WRONLY | O_CREAT | O_EXCL | O_APPEND, S_IRUSR | S_IWUSR | S_IRGRP | S_IROTH)
        }
        guard fd >= 0 else {
            return .failure(SessionLogError(reason: String(cString: strerror(errno))))
        }

        let log = SessionLog(path: finalPath, fd: fd)
        let args = arguments.isEmpty ? "(none)" : arguments.joined(separator: " ")
        let firstLine = "iairport session log  started \(localISO8601(now))  pid \(getpid())  args: \(args)\n"
        guard log.writeData(Data(firstLine.utf8)) else {
            let reason = String(cString: strerror(errno))
            log.close()
            _ = try? FileManager.default.removeItem(atPath: finalPath)
            return .failure(SessionLogError(reason: reason))
        }
        return .success(log)
    }

    public func write(_ line: String) {
        _ = writeData(Data((line + "\n").utf8))
    }

    public func close() {
        lock.lock()
        defer { lock.unlock() }
        guard !closed else { return }
        closed = true
        Darwin.close(fd)
    }

    private func writeData(_ data: Data) -> Bool {
        lock.lock()
        defer { lock.unlock() }
        guard !closed else { return false }
        var written = 0
        return data.withUnsafeBytes { rawBuffer in
            guard let base = rawBuffer.baseAddress else { return true }
            while written < rawBuffer.count {
                let result = Darwin.write(fd, base.advanced(by: written), rawBuffer.count - written)
                if result < 0 {
                    if errno == EINTR { continue }
                    return false
                }
                if result == 0 { return false }
                written += result
            }
            return true
        }
    }

    private static func localISO8601(_ date: Date) -> String {
        let formatter = DateFormatter()
        formatter.locale = Locale(identifier: "en_US_POSIX")
        formatter.timeZone = .current
        formatter.dateFormat = "yyyy-MM-dd'T'HH:mm:ssXXXXX"
        return formatter.string(from: date)
    }

    private static func posixReason(from error: Error) -> String {
        let nsError = error as NSError
        if nsError.domain == NSPOSIXErrorDomain {
            return String(cString: strerror(Int32(nsError.code)))
        }
        if let underlying = nsError.userInfo[NSUnderlyingErrorKey] as? Error {
            return posixReason(from: underlying)
        }
        return error.localizedDescription
    }
}
