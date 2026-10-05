import Foundation
import Darwin
import IAirportCore

// Returns the path dyld used to start this process and its realpath.
// They differ when the process was started through a symlink.
func executablePaths() -> (raw: String, resolved: String)? {
    var size: UInt32 = 0
    _ = _NSGetExecutablePath(nil, &size)
    var buffer = [CChar](repeating: 0, count: Int(size))
    guard _NSGetExecutablePath(&buffer, &size) == 0 else { return nil }
    let raw = String(cString: buffer)
    guard let resolved = buffer.withUnsafeBufferPointer({ realpath($0.baseAddress, nil) }) else {
        return (raw, raw)
    }
    defer { free(resolved) }
    return (raw, String(cString: resolved))
}

func execPath(_ path: String, arguments: [String]) -> Bool {
    var cArgs: [UnsafeMutablePointer<CChar>?] = ([path] + arguments).map { strdup($0) }
    cArgs.append(nil)
    execv(path, &cArgs)
    let execError = errno
    for arg in cArgs where arg != nil { free(arg) }
    let message = String(cString: strerror(execError))
    FileHandle.standardError.write(Data("error: re-exec failed: \(message)\n".utf8))
    return false
}

let launchArguments = Array(CommandLine.arguments.dropFirst())
let startPaths = executablePaths()
let executablePath = startPaths?.resolved ?? CommandLine.arguments.first ?? "/usr/local/bin/iairport"

if let startPaths,
   BundleLocator.shouldReexec(argv0: startPaths.raw, resolved: startPaths.resolved) {
    _ = execPath(startPaths.resolved, arguments: launchArguments)
}

if CommandLine.arguments.dropFirst().contains("--handshake") {
    exit(0)
}

// Child of the root helper: drop to the invoking user before anything else.
if geteuid() == 0,
   let raw = getenv(DropTarget.environmentKey).map({ String(cString: $0) }) {
    _ = unsetenv(DropTarget.environmentKey)
    guard let target = DropTarget.decode(raw), target.apply() else {
        FileHandle.standardError.write(Data("error: could not drop privileges to the invoking user\n".utf8))
        exit(1)
    }
}

// `sudo iairport`: stay root as the wdutil helper and run the monitor as the user.
if geteuid() == 0,
   let sudoUID = getenv("SUDO_UID").flatMap({ uid_t(String(cString: $0)) }),
   sudoUID != 0,
   getenv("IAIRPORT_SUDO") == nil {
    switch RootHelperServer.launchUserChild(executable: executablePath, arguments: launchArguments, uid: sudoUID) {
    case .success(let child):
        RootHelperServer.serve(fd: child.fd, childPID: child.pid)
    case .failure(let error):
        FileHandle.standardError.write(Data("warning: could not start the user-level monitor (\(error)); running as root in cache mode\n".utf8))
    }
}

switch CLIParser.parse(launchArguments) {
case .failure(let message):
    FileHandle.standardError.write(Data("error: \(message)\n\(CLIParser.helpText())".utf8))
    exit(2)
case .success(let options):
    if options.help {
        print(CLIParser.helpText())
        exit(0)
    }
    if options.debugToggle {
        exit(WdutilInfo.toggleDebug())
    }
    var activeMonitor: IAirportMonitor?
    let gate = LocationGateRuntime(interfaceName: options.interfaceName ?? LinkReader.defaultInterfaceName(), executablePath: executablePath, jsonMode: options.json)
    gate.start { result in
        let monitor = IAirportMonitor(options: options, executablePath: executablePath, locationGate: result)
        monitor.onLiveSourceLost = { gate.requestPromptAgain() }
        activeMonitor = monitor
        SignalInstaller.install {
            // First signal: clean shutdown on the state queue. Second signal:
            // the queue is stuck, so kill children and leave right away.
            if SignalInstaller.recordSignal() > 1 {
                WdutilInfo.terminateChildren()
                _exit(130)
            }
            monitor.requestShutdown()
        }
        monitor.start()
    }
    _ = activeMonitor
    dispatchMain()
}
