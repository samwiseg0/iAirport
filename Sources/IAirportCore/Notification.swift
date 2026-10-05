import Foundation
import UserNotifications

public enum NotificationRoute: Equatable {
    case userNotifications
    case osascript
}

public enum NotificationAction: Equatable {
    case post
    case requestThenPost
    case skip
}

public enum NotificationDecision {
    /// UNUserNotificationCenter aborts the process when Bundle.main has no identifier,
    /// so a plain `.build/release/iairport` run keeps the osascript path.
    public static func route(bundleIdentifier: String?) -> NotificationRoute {
        bundleIdentifier == nil ? .osascript : .userNotifications
    }

    public static func action(for status: UNAuthorizationStatus) -> NotificationAction {
        switch status {
        case .notDetermined: return .requestThenPost
        case .authorized, .provisional: return .post
        case .denied: return .skip
        @unknown default: return .skip
        }
    }
}

public enum Notifier {
    public static let deniedNote = "Roam notifications are off for iairport. Allow them in System Settings > Notifications > iairport, or pass --no-notify."

    /// Posts a roam banner. `note` receives at most one line per run, when
    /// notifications are denied or when macOS rejects the app bundle. It may be
    /// called on any queue.
    public static func post(title: String, message: String, enabled: Bool, note: @escaping (String) -> Void = { _ in }) {
        guard enabled else { return }
        let pending = Pending(title: title, message: message, note: note)
        lock.lock()
        if route == nil { route = NotificationDecision.route(bundleIdentifier: Bundle.main.bundleIdentifier) }
        let current = route
        lock.unlock()
        switch current {
        case .userNotifications:
            postWithUserNotifications(pending)
        case .osascript, nil:
            postWithOsascript(pending)
        }
    }

    private struct Pending {
        let title: String
        let message: String
        let note: (String) -> Void
    }

    private static let lock = NSLock()
    private static var route: NotificationRoute?
    private static var requestInFlight = false
    private static var latestPending: Pending?
    private static var noted = false

    // Posting from the bundle puts the iairport icon on the banner. osascript
    // banners carry Script Editor's icon.
    private static let center: UNUserNotificationCenter = {
        let center = UNUserNotificationCenter.current()
        center.delegate = bannerDelegate
        return center
    }()
    private static let bannerDelegate = BannerDelegate()

    private static func postWithUserNotifications(_ pending: Pending) {
        center.getNotificationSettings { settings in
            switch NotificationDecision.action(for: settings.authorizationStatus) {
            case .skip:
                noteOnce(deniedNote, via: pending.note)
            case .post:
                deliver(pending)
            case .requestThenPost:
                requestAuthorization(then: pending)
            }
        }
    }

    // One prompt per run. Roams that arrive while the prompt is up collapse
    // into the latest one, so Allow does not release a burst of stale banners.
    private static func requestAuthorization(then pending: Pending) {
        lock.lock()
        latestPending = pending
        let alreadyRequesting = requestInFlight
        requestInFlight = true
        lock.unlock()
        guard !alreadyRequesting else { return }
        center.requestAuthorization(options: [.alert]) { granted, error in
            lock.lock()
            let latest = latestPending
            latestPending = nil
            requestInFlight = false
            lock.unlock()
            guard let latest else { return }
            if let error {
                fallBack(after: error, pending: latest)
            } else if granted {
                deliver(latest)
            } else {
                noteOnce(deniedNote, via: latest.note)
            }
        }
    }

    private static func deliver(_ pending: Pending) {
        let content = UNMutableNotificationContent()
        content.title = pending.title
        content.body = pending.message
        let request = UNNotificationRequest(identifier: UUID().uuidString, content: content, trigger: nil)
        center.add(request) { error in
            if let error {
                fallBack(after: error, pending: pending)
            }
        }
    }

    // usernoted refuses bundles Launch Services has marked launch-disabled, the
    // same check locationd applies. Stay on osascript for the rest of the run so
    // the banners keep one look.
    private static func fallBack(after error: Error, pending: Pending) {
        lock.lock()
        route = .osascript
        lock.unlock()
        noteOnce("macOS rejected notifications from this app bundle (\(error.localizedDescription)). Roam banners use osascript for this run and carry Script Editor's icon. On managed Macs install with `sudo make install APPINSTALLDIR=/Applications`.", via: pending.note)
        postWithOsascript(pending)
    }

    private static func noteOnce(_ line: String, via note: (String) -> Void) {
        lock.lock()
        let first = !noted
        noted = true
        lock.unlock()
        if first { note(line) }
    }

    private static func postWithOsascript(_ pending: Pending) {
        let process = Process()
        process.executableURL = URL(fileURLWithPath: "/usr/bin/osascript")
        let escapedMessage = pending.message.replacingOccurrences(of: "\\", with: "\\\\").replacingOccurrences(of: "\"", with: "\\\"")
        let escapedTitle = pending.title.replacingOccurrences(of: "\\", with: "\\\\").replacingOccurrences(of: "\"", with: "\\\"")
        process.arguments = ["-e", "display notification \"\(escapedMessage)\" with title \"\(escapedTitle)\""]
        process.standardOutput = Pipe()
        process.standardError = Pipe()
        try? process.run()
    }
}

// Without a delegate macOS hides banners while the posting app counts as
// foreground. The monitor owns the terminal, so it can.
private final class BannerDelegate: NSObject, UNUserNotificationCenterDelegate {
    func userNotificationCenter(_ center: UNUserNotificationCenter, willPresent notification: UNNotification, withCompletionHandler completionHandler: @escaping (UNNotificationPresentationOptions) -> Void) {
        completionHandler([.banner, .list])
    }
}
