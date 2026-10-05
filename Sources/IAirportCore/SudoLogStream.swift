import Foundation
import Darwin

/// No-op SIGINT handler used while sudo owns the prompt.
private func sudoPromptSIGINT(_ signal: Int32) {}

// Plain `iairport` on an account that is not an admin, where `log stream`
// refuses to run. iairport asks sudo once, at startup, for exactly
// `/usr/bin/log stream --predicate 'process == "airportd"' --info --style compact`
// and reads its stdout as the user. Nothing else runs as root, and the sudo
// policy sees and audits that exact command.
//
// sudo prompts on /dev/tty, but sudo policy plugins (BeyondTrust, for one)
// write some prompt text, such as a reason menu, to stdout or stderr. Until
// `log stream` starts, iairport copies both to the terminal so the prompt
// stays readable. The first `log stream` line marks the hand-over: from there
// on stdout is log data for LogStreamTail.
public final class SudoLogStream {
    public struct Tools: Equatable {
        public var sudo: String
        public var log: String

        public static let system = Tools(sudo: "/usr/bin/sudo", log: "/usr/bin/log")

        public init(sudo: String, log: String) {
            self.sudo = sudo
            self.log = log
        }
    }

    public private(set) static var shared: SudoLogStream?

    private let child: ForegroundChild
    private let lock = NSLock()
    private var initialData: Data
    private var handedOut = false

    private init(child: ForegroundChild, initialData: Data) {
        self.child = child
        self.initialData = initialData
    }

    // MARK: Decision

    /// True when plain `iairport` should ask sudo for `log stream` at startup.
    public static func shouldOffer(noSudo: Bool) -> Bool {
        guard !noSudo, geteuid() != 0, RootHelperClient.shared == nil else { return false }
        guard isatty(STDIN_FILENO) == 1, isatty(STDERR_FILENO) == 1 else { return false }
        return !currentUserIsAdmin()
    }

    static func currentUserIsAdmin() -> Bool {
        guard let entry = getpwuid(getuid()), let admin = getgrnam("admin") else { return false }
        let adminGID = Int32(bitPattern: admin.pointee.gr_gid)
        var count: Int32 = 64
        var groups = [Int32](repeating: 0, count: Int(count))
        while getgrouplist(entry.pointee.pw_name, Int32(bitPattern: entry.pointee.pw_gid), &groups, &count) == -1 {
            guard count < 4096 else { return false }
            count *= 2
            groups = [Int32](repeating: 0, count: Int(count))
        }
        return groups.prefix(Int(count)).contains(adminGID)
    }

    // MARK: Hand-over detection

    static let bannerPrefix = "Filtering the log data"

    /// True for the first lines `log stream` prints: the filter banner, the
    /// column header, or a log line.
    static func isLogStreamLine(_ line: String) -> Bool {
        if line.hasPrefix(bannerPrefix) { return true }
        if line.hasPrefix("Timestamp ") { return true }
        return line.range(of: "^\\d{4}-\\d{2}-\\d{2} \\d{2}:\\d{2}:\\d{2}", options: .regularExpression) != nil
    }

    /// Splits stdout that arrived before the hand-over. Returns the prompt
    /// text to show, and the log data from the first `log stream` output on.
    /// `logData` is nil while no `log stream` line has appeared.
    ///
    /// A policy prompt such as "Select a reason: " ends without a newline,
    /// because the answer goes to the terminal, so the banner can follow it
    /// on the same stdout line. Split there, not only at line starts.
    static func split(_ pending: Data) -> (prompt: Data, logData: Data?) {
        let banner = Data(bannerPrefix.utf8)
        var index = pending.startIndex
        while let newline = pending[index...].firstIndex(of: UInt8(ascii: "\n")) {
            if let found = pending[index..<newline].range(of: banner) {
                return (Data(pending[pending.startIndex..<found.lowerBound]), Data(pending[found.lowerBound...]))
            }
            let line = String(decoding: pending[index..<newline], as: UTF8.self)
            if isLogStreamLine(line) {
                return (Data(pending[pending.startIndex..<index]), Data(pending[index...]))
            }
            index = pending.index(after: newline)
        }
        return (pending, nil)
    }

    /// Length of the longest suffix of `data` that is a proper prefix of the
    /// banner.
    static func bannerPrefixOverlap(_ data: Data) -> Int {
        let banner = Array(bannerPrefix.utf8)
        let bytes = Array(data.suffix(banner.count - 1))
        var length = min(bytes.count, banner.count - 1)
        while length > 0 {
            if Array(bytes.suffix(length)) == Array(banner.prefix(length)) { return length }
            length -= 1
        }
        return 0
    }

    // MARK: Start

    /// Runs `sudo log stream` and waits until it streams, the user cancels, or
    /// sudo refuses. Returns nil unless `log stream` is running as root.
    @discardableResult
    public static func start(tools: Tools = .system, promptOutput: FileHandle = .standardError, timeout: TimeInterval = 600, note: (String) -> Void) -> SudoLogStream? {
        // Ctrl-C at the prompt should cancel sudo, not iairport. sudo shares
        // iairport's process group, so both get the SIGINT; iairport catches
        // it with a no-op until sudo is done with the terminal.
        let previousINT = signal(SIGINT, sudoPromptSIGINT)
        defer { signal(SIGINT, previousINT) }
        guard let child = ForegroundChild.spawn(executable: tools.sudo, arguments: [tools.log] + LogStreamTail.streamArguments) else {
            note("Could not run sudo (\(String(cString: strerror(errno)))); continuing without root.")
            return nil
        }

        let outFD = child.stdoutFD
        let errFD = child.stderrFD
        var pending = Data()
        var shown = 0
        var outOpen = true
        var errOpen = true
        var buffer = [UInt8](repeating: 0, count: 16384)
        let deadline = Date().addingTimeInterval(timeout)

        func show(_ data: Data) {
            guard !data.isEmpty else { return }
            promptOutput.write(data)
        }

        while Date() < deadline && (outOpen || errOpen) {
            var fds: [pollfd] = []
            if outOpen { fds.append(pollfd(fd: outFD, events: Int16(POLLIN), revents: 0)) }
            if errOpen { fds.append(pollfd(fd: errFD, events: Int16(POLLIN), revents: 0)) }
            let ready = poll(&fds, nfds_t(fds.count), 200)
            if ready < 0 && errno != EINTR { break }
            for entry in fds where entry.revents != 0 {
                let count = read(entry.fd, &buffer, buffer.count)
                if count <= 0 {
                    if count == 0 || errno != EINTR && errno != EAGAIN {
                        if entry.fd == outFD { outOpen = false } else { errOpen = false }
                    }
                    continue
                }
                let chunk = Data(buffer[0..<count])
                if entry.fd == errFD {
                    show(chunk)
                    continue
                }
                pending.append(chunk)
                let parts = split(pending)
                if let logData = parts.logData {
                    // Show the rest of the prompt text and hand everything
                    // from the first log line on over. If part of that log
                    // line was already shown as a partial line, end it.
                    if parts.prompt.count > shown {
                        show(parts.prompt.suffix(from: parts.prompt.startIndex + shown))
                    } else if shown > parts.prompt.count {
                        show(Data("\n".utf8))
                    }
                    let stream = SudoLogStream(child: child, initialData: logData)
                    stream.drainStderr()
                    shared = stream
                    atexit { SudoLogStream.shared?.stop() }
                    return stream
                }
                // Show prompt text as it arrives, including a partial line
                // such as "Select a reason: ". Hold back a tail that could be
                // the start of the log stream banner.
                let visible = pending.count - bannerPrefixOverlap(pending)
                if visible > shown {
                    show(pending[(pending.startIndex + shown)..<(pending.startIndex + visible)])
                    shown = visible
                }
            }
        }

        if pending.count > shown { show(pending.suffix(from: pending.startIndex + shown)) }
        // Both pipes closed means sudo is exiting: reap it for the status.
        if let status = child.exitStatus(wait: !outOpen && !errOpen) {
            child.closeFDs()
            note("sudo did not start log stream (exit \(status)); continuing without root. --no-sudo skips this prompt.")
        } else {
            kill(child.pid, SIGTERM)
            _ = child.exitStatus(wait: true)
            child.closeFDs()
            note("sudo did not start log stream in time; continuing without root.")
        }
        return nil
    }

    // MARK: Consumer

    /// A dup of the `log stream` stdout fd, which the caller owns and closes,
    /// and the log bytes read before the hand-over. Handed out once.
    public func takeStream() -> (fd: Int32, initial: Data)? {
        lock.lock()
        defer { lock.unlock() }
        guard !handedOut else { return nil }
        let fd = dup(child.stdoutFD)
        guard fd >= 0 else { return nil }
        _ = fcntl(fd, F_SETFD, FD_CLOEXEC)
        handedOut = true
        let data = initialData
        initialData = Data()
        return (fd, data)
    }

    /// Stops `log stream`. sudo forwards SIGTERM to the command. sudo keeps the
    /// invoking user as its real uid, so the user may signal it.
    public func stop() {
        guard child.exitStatus(wait: false) == nil else { return }
        kill(child.pid, SIGTERM)
    }

    // sudo or the policy plugin may still write to stderr. Keep the pipe
    // drained so sudo never blocks on it.
    private func drainStderr() {
        let fd = child.stderrFD
        Thread.detachNewThread {
            var buffer = [UInt8](repeating: 0, count: 4096)
            while true {
                let count = read(fd, &buffer, buffer.count)
                if count < 0 && errno == EINTR { continue }
                if count <= 0 { break }
            }
        }
    }
}

/// A child spawned in the caller's process group, with stdin from /dev/null
/// and stdout and stderr on pipes. Foundation's Process puts each child in a
/// new process group. A sudo there is a background job: it cannot own the
/// terminal for its prompt, and Ctrl-C never reaches it.
final class ForegroundChild {
    let pid: pid_t
    let stdoutFD: Int32
    let stderrFD: Int32
    private let lock = NSLock()
    private var status: Int32?

    private init(pid: pid_t, stdoutFD: Int32, stderrFD: Int32) {
        self.pid = pid
        self.stdoutFD = stdoutFD
        self.stderrFD = stderrFD
    }

    static func spawn(executable: String, arguments: [String]) -> ForegroundChild? {
        var outPipe: [Int32] = [-1, -1]
        var errPipe: [Int32] = [-1, -1]
        guard pipe(&outPipe) == 0 else { return nil }
        guard pipe(&errPipe) == 0 else {
            close(outPipe[0]); close(outPipe[1])
            return nil
        }
        _ = fcntl(outPipe[0], F_SETFD, FD_CLOEXEC)
        _ = fcntl(errPipe[0], F_SETFD, FD_CLOEXEC)

        var actions: posix_spawn_file_actions_t?
        posix_spawn_file_actions_init(&actions)
        defer { posix_spawn_file_actions_destroy(&actions) }
        posix_spawn_file_actions_addopen(&actions, STDIN_FILENO, "/dev/null", O_RDONLY, 0)
        posix_spawn_file_actions_adddup2(&actions, outPipe[1], STDOUT_FILENO)
        posix_spawn_file_actions_adddup2(&actions, errPipe[1], STDERR_FILENO)

        var attr: posix_spawnattr_t?
        posix_spawnattr_init(&attr)
        defer { posix_spawnattr_destroy(&attr) }
        // No POSIX_SPAWN_SETPGROUP: stay in the terminal's foreground group.
        // Reset signals iairport may ignore, and close every other fd.
        var defaults = sigset_t()
        sigemptyset(&defaults)
        for number in [SIGINT, SIGTERM, SIGHUP, SIGPIPE, SIGQUIT, SIGTSTP] { sigaddset(&defaults, number) }
        var mask = sigset_t()
        sigemptyset(&mask)
        posix_spawnattr_setsigdefault(&attr, &defaults)
        posix_spawnattr_setsigmask(&attr, &mask)
        posix_spawnattr_setflags(&attr, Int16(POSIX_SPAWN_SETSIGDEF | POSIX_SPAWN_SETSIGMASK | POSIX_SPAWN_CLOEXEC_DEFAULT))

        var argv: [UnsafeMutablePointer<CChar>?] = ([executable] + arguments).map { strdup($0) }
        argv.append(nil)
        defer { for pointer in argv where pointer != nil { free(pointer) } }

        var pid: pid_t = 0
        let rc = posix_spawn(&pid, executable, &actions, &attr, argv, environ)
        close(outPipe[1])
        close(errPipe[1])
        guard rc == 0 else {
            close(outPipe[0])
            close(errPipe[0])
            errno = rc
            return nil
        }
        return ForegroundChild(pid: pid, stdoutFD: outPipe[0], stderrFD: errPipe[0])
    }

    /// Exit status, or 128 + signal. Nil while the child runs and `wait` is false.
    func exitStatus(wait: Bool) -> Int32? {
        lock.lock()
        defer { lock.unlock() }
        if let status { return status }
        var raw: Int32 = 0
        while true {
            let result = waitpid(pid, &raw, wait ? 0 : WNOHANG)
            if result == pid { break }
            if result == 0 { return nil }
            if errno == EINTR { continue }
            return nil
        }
        let signaled = raw & 0x7f
        status = signaled == 0 ? (raw >> 8) & 0xff : 128 + signaled
        return status
    }

    func closeFDs() {
        close(stdoutFD)
        close(stderrFD)
    }
}
