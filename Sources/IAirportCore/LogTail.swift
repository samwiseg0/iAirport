import Foundation
import Darwin

public final class LogStreamTail {
    /// Arguments for `/usr/bin/log`. The root helper uses the same ones.
    static let streamArguments = ["stream", "--predicate", "process == \"airportd\"", "--info", "--style", "compact"]

    private let queue: DispatchQueue
    private let renderer: Renderer
    private let onLine: (String) -> Void
    private let helper: RootHelperClient?
    private var process: Process?
    private var stdoutPipe: Pipe?
    private var stderrPipe: Pipe?
    private var helperHandle: FileHandle?
    private var stdoutBuffer = Data()
    private var stderrBuffer = Data()
    private var stopping = false
    private var restartAttempts = 0
    private var lastErrorLine: String?

    public init(queue: DispatchQueue, renderer: Renderer, helper: RootHelperClient? = RootHelperClient.shared, onLine: @escaping (String) -> Void) {
        self.queue = queue
        self.renderer = renderer
        self.helper = helper
        self.onLine = onLine
    }

    public func start() {
        stopping = false
        restartAttempts = 0
        // Root `log stream` from the startup sudo prompt, if it was granted.
        if let stream = SudoLogStream.shared?.takeStream() {
            if !stream.initial.isEmpty { append(data: stream.initial, isError: false) }
            attachHelperStream(fd: stream.fd)
            return
        }
        guard let helper else {
            launch()
            return
        }
        // Under `sudo iairport` the root helper runs `log stream`, because the
        // invoking user may not be an admin. The request is a socket round trip
        // that can queue behind a wdutil call, so keep it off the state queue.
        DispatchQueue.global(qos: .userInitiated).async { [weak self] in
            let fd = helper.startLogStream()
            self?.queue.async {
                guard let self, !self.stopping else { return }
                if let fd {
                    self.attachHelperStream(fd: fd)
                } else {
                    self.launch()
                }
            }
        }
    }

    public func stop() {
        stopping = true
        if let helperHandle {
            helperHandle.readabilityHandler = nil
            try? helperHandle.close()
            self.helperHandle = nil
        }
        guard let process else { return }
        stdoutPipe?.fileHandleForReading.readabilityHandler = nil
        stderrPipe?.fileHandleForReading.readabilityHandler = nil
        if process.isRunning {
            process.terminate()
            let deadline = Date().addingTimeInterval(2.0)
            while process.isRunning && Date() < deadline {
                usleep(50_000)
            }
            if process.isRunning {
                kill(process.processIdentifier, SIGKILL)
            }
            process.waitUntilExit()
        }
        self.process = nil
        stdoutPipe = nil
        stderrPipe = nil
    }

    // Reads the root helper's `log stream` output. The helper restarts the
    // stream itself, so EOF here means it gave up.
    private func attachHelperStream(fd: Int32) {
        let handle = FileHandle(fileDescriptor: fd, closeOnDealloc: true)
        helperHandle = handle
        handle.readabilityHandler = { [weak self] handle in
            let data = handle.availableData
            guard !data.isEmpty else {
                handle.readabilityHandler = nil
                self?.queue.async { self?.handleHelperEOF() }
                return
            }
            self?.queue.async { self?.append(data: data, isError: false) }
        }
    }

    private func handleHelperEOF() {
        guard !stopping else { return }
        helperHandle = nil
        // Ctrl-C reaches a sudo-run `log stream` too, so its EOF can land just
        // before shutdown starts. Wait a moment before calling it a failure.
        queue.asyncAfter(deadline: .now() + .seconds(1)) { [weak self] in
            guard let self, !self.stopping else { return }
            self.renderer.event(line: "warning: root log stream ended; continuing without log events", color: .yellow)
        }
    }

    private func launch() {
        let child = Process()
        child.executableURL = URL(fileURLWithPath: "/usr/bin/log")
        child.arguments = Self.streamArguments
        let stdout = Pipe()
        let stderr = Pipe()
        stdoutPipe = stdout
        stderrPipe = stderr
        child.standardOutput = stdout
        child.standardError = stderr
        stdout.fileHandleForReading.readabilityHandler = { [weak self] handle in
            let data = handle.availableData
            guard !data.isEmpty else {
                handle.readabilityHandler = nil
                return
            }
            self?.queue.async { self?.append(data: data, isError: false) }
        }
        stderr.fileHandleForReading.readabilityHandler = { [weak self] handle in
            let data = handle.availableData
            guard !data.isEmpty else {
                handle.readabilityHandler = nil
                return
            }
            self?.queue.async { self?.append(data: data, isError: true) }
        }
        child.terminationHandler = { [weak self] _ in
            self?.queue.async { self?.handleTermination() }
        }
        do {
            try child.run()
            process = child
        } catch {
            renderer.event(line: "warning: could not start log stream: \(error.localizedDescription)", color: .yellow)
        }
    }

    private func append(data: Data, isError: Bool) {
        // Pipe chunks can split a multibyte character, so split on bytes and
        // decode whole lines only.
        if isError {
            stderrBuffer.append(data)
            drain(buffer: &stderrBuffer, emit: false)
        } else {
            stdoutBuffer.append(data)
            drain(buffer: &stdoutBuffer, emit: true)
        }
    }

    private func drain(buffer: inout Data, emit: Bool) {
        while let newline = buffer.firstIndex(of: UInt8(ascii: "\n")) {
            let line = String(decoding: buffer[buffer.startIndex..<newline], as: UTF8.self)
            buffer.removeSubrange(buffer.startIndex...newline)
            if emit {
                onLine(line)
            } else if !line.trimmingCharacters(in: .whitespaces).isEmpty {
                lastErrorLine = line
            }
        }
    }

    /// True when `log stream` refused to run for a reason a restart cannot fix.
    static func isPermanentFailure(_ stderrLine: String?) -> Bool {
        guard let line = stderrLine?.lowercased() else { return false }
        return line.contains("must be admin") || line.contains("operation not permitted")
    }

    private func handleTermination() {
        guard !stopping else { return }
        stdoutPipe?.fileHandleForReading.readabilityHandler = nil
        stderrPipe?.fileHandleForReading.readabilityHandler = nil
        // Pick up a final stderr line that had no trailing newline.
        if !stderrBuffer.isEmpty {
            let tail = String(decoding: stderrBuffer, as: UTF8.self).trimmingCharacters(in: .whitespacesAndNewlines)
            if !tail.isEmpty { lastErrorLine = tail }
            stderrBuffer.removeAll()
        }
        process = nil
        stdoutPipe = nil
        stderrPipe = nil
        if Self.isPermanentFailure(lastErrorLine) {
            let reason = lastErrorLine.map { $0.trimmingCharacters(in: .whitespaces) } ?? ""
            renderer.event(line: "warning: log stream unavailable (\(reason)). It needs an admin account or root: run iairport in a terminal and approve its startup sudo prompt. Continuing without airportd log events: roam markers, roam reasons and join timing stay blank.", color: .yellow)
            return
        }
        restartAttempts += 1
        guard restartAttempts <= 5 else {
            renderer.event(line: "warning: log stream exited; continuing without log events", color: .yellow)
            return
        }
        let delay = min(1 << max(restartAttempts - 1, 0), 8)
        renderer.event(line: "warning: log stream exited; restarting in \(delay)s", color: .yellow)
        queue.asyncAfter(deadline: .now() + .seconds(delay)) { [weak self] in
            guard let self, !self.stopping else { return }
            self.launch()
        }
    }
}
