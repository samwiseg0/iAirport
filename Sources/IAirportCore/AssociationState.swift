import Foundation

public struct AssociationInfo: Equatable {
    public var bssid: String
    public var ssid: String?
    public var channel: Int?
    public var rssi: Int?
    public var since: Date
    public var bssidSource: BSSIDSource
    public var apName: String?

    public init(bssid: String, ssid: String? = nil, channel: Int? = nil, rssi: Int? = nil, since: Date, bssidSource: BSSIDSource = .cache, apName: String? = nil) {
        self.bssid = bssid
        self.ssid = ssid
        self.channel = channel
        self.rssi = rssi
        self.since = since
        self.bssidSource = bssidSource
        self.apName = apName
    }
}

public enum AssociationSnapshot: Equatable {
    case off
    case disconnected
    case associated(AssociationInfo)
}

public enum AssociationTransitionKind: String, Equatable {
    case join
    case roam
    case reconnect
    case disconnect
}

public struct AssociationTransition: Equatable {
    public var kind: AssociationTransitionKind
    public var old: AssociationInfo?
    public var new: AssociationInfo?
    public var at: Date
    public var dwell: TimeInterval?

    public init(kind: AssociationTransitionKind, old: AssociationInfo?, new: AssociationInfo?, at: Date, dwell: TimeInterval? = nil) {
        self.kind = kind
        self.old = old
        self.new = new
        self.at = at
        self.dwell = dwell
    }
}

public struct AssociationTracker {
    private var current: AssociationSnapshot?
    private var lastAssociated: AssociationInfo?
    public private(set) var generation: Int = 0

    public init() {}

    public var currentAssociation: AssociationInfo? {
        if case .associated(let info) = current { return info }
        return nil
    }

    /// Replaces the current association without reporting a transition. Used
    /// when the BSSID source changes and the old value came from a stale
    /// cache. The join time carries over, since the real link did not change.
    public mutating func rebaseline(_ info: AssociationInfo) {
        var corrected = info
        if let bssid = MACAddress.normalize(info.bssid) { corrected.bssid = bssid }
        if let old = currentAssociation { corrected.since = old.since }
        current = .associated(corrected)
        lastAssociated = corrected
    }

    public mutating func commit(_ snapshot: AssociationSnapshot, at date: Date) -> AssociationTransition? {
        let normalized = normalize(snapshot, at: date)
        defer { current = normalized }
        guard let prior = current else {
            if case .associated(let info) = normalized {
                generation += 1
                lastAssociated = info
                return AssociationTransition(kind: .join, old: nil, new: info, at: date)
            }
            current = normalized
            return nil
        }
        if equivalentAssociationState(prior, normalized) {
            if case .associated(let info) = normalized {
                lastAssociated = info
            }
            return nil
        }
        switch (prior, normalized) {
        case (.associated(let old), .associated(let new)):
            guard old.bssid != new.bssid else { return nil }
            generation += 1
            lastAssociated = new
            return AssociationTransition(kind: .roam, old: old, new: new, at: date, dwell: date.timeIntervalSince(old.since))
        case (.associated(let old), .disconnected), (.associated(let old), .off):
            generation += 1
            lastAssociated = old
            return AssociationTransition(kind: .disconnect, old: old, new: nil, at: date, dwell: date.timeIntervalSince(old.since))
        case (.disconnected, .associated(let new)), (.off, .associated(let new)):
            let oldAssociation = lastAssociated
            let kind: AssociationTransitionKind = oldAssociation?.bssid == new.bssid ? .reconnect : .join
            generation += 1
            lastAssociated = new
            return AssociationTransition(kind: kind, old: kind == .reconnect ? oldAssociation : nil, new: new, at: date)
        case (.off, .disconnected), (.disconnected, .off):
            return nil
        default:
            return nil
        }
    }

    private func normalize(_ snapshot: AssociationSnapshot, at date: Date) -> AssociationSnapshot {
        switch snapshot {
        case .associated(var info):
            guard let bssid = MACAddress.normalize(info.bssid) else { return .disconnected }
            info.bssid = bssid
            if case .associated(let currentInfo) = current, currentInfo.bssid == bssid {
                info.since = currentInfo.since
            } else {
                info.since = date
            }
            return .associated(info)
        default:
            return snapshot
        }
    }

    private func equivalentAssociationState(_ lhs: AssociationSnapshot, _ rhs: AssociationSnapshot) -> Bool {
        switch (lhs, rhs) {
        case (.off, .off), (.disconnected, .disconnected):
            return true
        case (.associated(let left), .associated(let right)):
            return left.bssid == right.bssid
        default:
            return false
        }
    }
}

public enum CacheModeRoam {
    public static func transitionFromDriverSignal(old: AssociationInfo, cachedBSSIDAfterSettle: String?, at date: Date) -> AssociationTransition? {
        guard let cached = MACAddress.normalize(cachedBSSIDAfterSettle), cached == old.bssid else { return nil }
        let new = AssociationInfo(bssid: "?", ssid: old.ssid, since: date, bssidSource: .cache)
        return AssociationTransition(kind: .roam, old: old, new: new, at: date, dwell: date.timeIntervalSince(old.since))
    }
}

public extension AssociationSnapshot {
    static func from(sample: LinkSample) -> AssociationSnapshot {
        switch sample.status {
        case .off:
            return .off
        case .disconnected:
            return .disconnected
        case .associated:
            guard let bssid = sample.bssid else { return .disconnected }
            return .associated(AssociationInfo(bssid: bssid, ssid: sample.ssid, channel: sample.channel, rssi: sample.rssiDBM, since: sample.timestamp, bssidSource: sample.bssidSource, apName: sample.apName))
        }
    }
}
