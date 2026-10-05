import Foundation
import Darwin

public final class IAirportMonitor {
    private let options: RunOptions
    private let interfaceName: String
    private let queue = DispatchQueue(label: "iairport.state")
    private let renderer: Renderer
    private let time = TimeFormatter()
    private let oui: OUI
    private let ouiWarning: String?
    private let linkReader: LinkReader
    private let apNames: APNameResolver
    private let ipReader: IPStateReader
    private var bssidSource: BSSIDSource
    private var cacheReason: LocationCacheReason?
    private var canRecheckLiveSource: Bool
    private let protectedFolder: String?
    private var cacheWarningPrinted = false
    private var liveSourceLinePrinted = false
    public var onLiveSourceLost: (() -> Bool)?
    private var wdutilPrivilege: WdutilInfo.Privilege = .unavailable
    private var wdutilHintPrinted = false
    private var csv: CSVLogger?
    private var logTail: LogStreamTail?
    private var dynamicStore: DynamicStoreWatcher?
    private var coreWLANBridge: CoreWLANEventBridge?
    private var timer: DispatchSourceTimer?
    private var wdutilTimer: DispatchSourceTimer?
    private var association = AssociationTracker()
    private var counters = CounterTracker()
    private var metrics = LinkMetrics()
    private var lastSample: LinkSample?
    private var lastObservedIP: IPState?
    private var lastIPLine: String?
    private var pendingIPEvent: IPState?
    private var ipLastChangedAt: Date?
    private var pendingRoamIP: PendingRoamIP?
    private var pendingRoamCSV = PendingRoamCSVBuffer()
    private var pendingJoinTiming: (timing: JoinTiming, at: Date)?
    private var joinTimingGate: JoinTimingGate?
    private var roamRequestCapture: RoamRequestCapture?
    private var pendingTransitionIP: PendingTransitionIP?
    private var dhcpService: String?
    private var dhcpv6Service: String?
    private var dhcpDeadline: Date?
    private var dhcpv6Deadline: Date?
    private var wdutilInFlight = false
    private var shuttingDown = false
    private var startDate = Date()
    private var roams = 0
    private var reconnects = 0
    private var disconnects = 0
    private var distinctBSSIDs = Set<String>()
    private var history: [RoamHistoryEntry] = []
    private var currentHistoryIndex: Int?
    private var driverSignal: DriverSignal?
    private var pendingCacheRoam: AssociationInfo?
    private var unresolvedCacheRoamFrom: String?
    private var firstWithheldAt: Date?

    public init(options: RunOptions, executablePath: String?, locationGate: LocationGateResult) {
        self.options = options
        interfaceName = options.interfaceName ?? LinkReader.defaultInterfaceName()
        renderer = Renderer(noColor: options.noColor, jsonMode: options.json)
        bssidSource = locationGate.source
        cacheReason = locationGate.cacheReason
        canRecheckLiveSource = locationGate.canRecheck
        protectedFolder = locationGate.protectedFolder
        let loaded = OUI.load(explicitPath: options.ouiPath, executablePath: executablePath)
        oui = loaded.0
        ouiWarning = loaded.1
        linkReader = LinkReader(interfaceName: interfaceName)
        apNames = APNameResolver(interfaceName: interfaceName)
        ipReader = IPStateReader(interfaceName: interfaceName)
        renderer.onOutputClosed = { [weak self] in
            self?.requestShutdown()
        }
    }

    public func start() {
        queue.async {
            self.startDate = Date()
            self.joinTimingGate = JoinTimingGate(launchDate: self.startDate)
            self.printHeader()
            self.configureWdutilPrivilege()
            var warnings: [String] = []
            self.csv = CSVLogger(enabled: self.options.log, time: self.time, warnings: &warnings)
            for warning in warnings { self.writeEvent(warning, color: .yellow, type: "log") }
            self.setupWatchers()
            self.setupTimers()
            self.sampleAndCommit(reason: "startup")
            self.logTail = LogStreamTail(queue: self.queue, renderer: self.renderer) { [weak self] line in
                self?.handleLogLine(line)
            }
            self.logTail?.start()
        }
    }

    public func requestShutdown() {
        queue.async {
            self.shutdown()
        }
    }

    private func printHeader() {
        guard !options.json else { return }
        renderer.event(line: "iairport v2.0.0 (Swift rewrite of iAirport by Guillaume Germain)")
        renderer.event(line: macOSLine())
        renderer.event(line: "interface \(interfaceName)")
        if let ouiWarning { renderer.event(line: "warning: \(ouiWarning)", color: .yellow) }
        if options.log {
            renderer.event(line: "Logging to iairport-samples.csv, iairport-roams.csv and bssid_list.txt")
        }
        printCacheWarningIfNeeded()
    }

    private func printCacheWarningIfNeeded() {
        guard !cacheWarningPrinted, bssidSource == .cache, let cacheReason else { return }
        cacheWarningPrinted = true
        switch cacheReason {
        case .notBundled:
            renderer.event(line: "Live SSID/BSSID need the app bundle. Run `make` and use build/iairport.app/Contents/MacOS/iairport, or `make install`.", color: .yellow)
        case .noGrant:
            if let protectedFolder {
                renderer.event(line: "Location is granted, but the app bundle is under \(protectedFolder), which macOS protects. locationd cannot identify it there. Run `sudo make install` and use /usr/local/bin/iairport.", color: .yellow)
            } else {
                renderer.event(line: "Location not granted. BSSID comes from the scan cache and can lag after a join. Allow it in System Settings > Privacy & Security > Location Services > iairport.", color: .yellow)
            }
        }
    }

    // The probe can run `sudo -n`, so keep it off the state queue. A slow or
    // prompting sudo must not block sampling or Ctrl-C.
    private func configureWdutilPrivilege() {
        DispatchQueue.global(qos: .userInitiated).async { [weak self] in
            let privilege = WdutilInfo.effectivePrivilege()
            guard let self else { return }
            self.queue.async {
                guard !self.shuttingDown else { return }
                self.wdutilPrivilege = privilege
                if self.options.verbose {
                    self.renderer.event(line: privilege.description, color: .dim)
                }
                self.startWdutilTimer()
            }
        }
    }

    private func startWdutilTimer() {
        guard wdutilPrivilege != .unavailable, wdutilTimer == nil else { return }
        let wd = DispatchSource.makeTimerSource(queue: queue)
        wd.schedule(deadline: .now(), repeating: .seconds(5))
        wd.setEventHandler { [weak self] in self?.pollWdutil() }
        wd.resume()
        wdutilTimer = wd
    }

    private func setupWatchers() {
        coreWLANBridge = CoreWLANEventBridge(interfaceName: interfaceName, queue: queue) { [weak self] reason in
            self?.scheduleSettleReads(reason: reason)
        }
        coreWLANBridge?.start()
        dynamicStore = DynamicStoreWatcher(interfaceName: interfaceName, queue: queue) { [weak self] keys in
            self?.handleStoreKeys(keys)
        }
        dynamicStore?.start()
    }

    private func setupTimers() {
        let timer = DispatchSource.makeTimerSource(queue: queue)
        let interval = DispatchTimeInterval.milliseconds(max(1, Int(options.interval * 1000.0)))
        timer.schedule(deadline: .now() + interval, repeating: interval)
        timer.setEventHandler { [weak self] in self?.sampleAndCommit(reason: "poll") }
        timer.resume()
        self.timer = timer
    }

    private func handleStoreKeys(_ keys: [String]) {
        if keys.contains(where: { $0.contains("/AirPort") }) {
            scheduleSettleReads(reason: "sc-airport")
        }
        if keys.contains(where: { $0.contains("/IPv4") || $0.contains("/IPv6") || $0.contains("/DHCP") || $0.contains("/DHCPv6") }) {
            let beforeIP = lastObservedIP
            let hasDHCPv6 = keys.contains(where: { $0.contains("/DHCPv6") })
            let hasIPv6 = keys.contains(where: { $0.contains("/IPv6") })
            sampleAndCommit(reason: "sc-ip")
            if hasDHCPv6 {
                scheduleDHCPv6(service: serviceID(from: keys.first { $0.contains("/DHCPv6") }))
            } else if keys.contains(where: { $0.contains("/DHCP") }) {
                scheduleDHCP(service: serviceID(from: keys.first { $0.contains("/DHCP") }))
            } else if hasIPv6, let beforeIP, beforeIP != lastObservedIP {
                let line = "IPv6 RA/SLAAC update"
                writeEvent(line, color: .cyan, type: "ip", json: OutputFormatter.eventJSON(type: "ip", message: line, date: Date(), time: time))
            }
        }
    }

    private func scheduleSettleReads(reason: String) {
        if reason.contains("driver") {
            driverSignal = DriverSignal(bssid: lastSample?.bssid, at: Date())
        }
        for ms in [0, 500, 2000, 3000] {
            queue.asyncAfter(deadline: .now() + .milliseconds(ms)) { [weak self] in
                self?.sampleAndCommit(reason: reason)
            }
        }
        if reason.contains("driver") {
            queue.asyncAfter(deadline: .now() + .milliseconds(3100)) { [weak self] in
                self?.checkDriverSignal()
            }
        }
    }

    private func sampleAndCommit(reason: String) {
        guard !shuttingDown else { return }
        let emitStatus = reason == "startup" || reason == "poll"
        recheckLiveSourceIfNeeded()
        let ip = ipReader.read()
        let throughput = counters.sample(CounterReader.counters(for: interfaceName), now: Date())
        var sample = linkReader.read(metrics: metrics, ipState: ip, throughput: throughput, oui: oui, bssidSource: bssidSource)
        // Settle reads land mid-transition, where one nil bssid() is normal,
        // and several of them run within milliseconds of each other. Only a
        // withheld sample at least 500 ms after the first one means airportd
        // is refusing.
        if sample.liveBSSIDWithheld {
            if let first = firstWithheldAt, sample.timestamp.timeIntervalSince(first) >= 0.5 {
                firstWithheldAt = nil
                downgradeToCacheSource()
                sample = linkReader.read(metrics: metrics, ipState: ip, throughput: throughput, oui: oui, bssidSource: bssidSource)
            } else if firstWithheldAt == nil {
                firstWithheldAt = sample.timestamp
            }
        } else {
            firstWithheldAt = nil
        }
        if let lqmRate = metricsFromLQMRate(), sample.txRateMbps == nil || sample.txRateMbps == 0 {
            sample.txRateMbps = lqmRate
        }
        if sample.status == .associated, let bssid = sample.bssid, bssid != "?" {
            // A BSSID change forces a scan-cache read so the ROAM line and the
            // notification carry the new AP's name.
            let changed = association.currentAssociation?.bssid != bssid
            sample.apName = apNames.resolve(bssid, force: changed, now: sample.timestamp)
        }
        let beforeIP = lastObservedIP
        let transition = association.commit(.from(sample: sample), at: sample.timestamp)
        if transition != nil {
            metrics.clearBSSIDScoped()
        }
        if sample.status == .associated, let bssid = sample.bssid {
            distinctBSSIDs.insert(bssid)
        }
        handleIPObservation(sample.ipState, now: sample.timestamp, transition: transition, beforeIP: beforeIP)
        if let transition {
            handleTransition(transition, sample: sample, beforeIP: beforeIP)
        }
        lastSample = sample
        if emitStatus {
            csv?.writeSample(sample)
            if options.json {
                renderer.json(OutputFormatter.sampleJSON(sample: sample, time: time))
            } else {
                let rendered = OutputFormatter.status(sample: sample, verbose: options.verbose, time: time)
                renderer.status(line: rendered.0, color: rendered.1)
            }
        }
        flushPendingIP(now: sample.timestamp)
        flushPendingRoamIP(now: sample.timestamp)
        flushTransitionIP(now: sample.timestamp)
        flushDHCP(now: sample.timestamp)
    }

    private func recheckLiveSourceIfNeeded() {
        guard bssidSource == .cache, canRecheckLiveSource, linkReader.liveBSSIDAvailable() else { return }
        bssidSource = .live
        if !liveSourceLinePrinted {
            liveSourceLinePrinted = true
            writeEvent("Live SSID/BSSID available.", color: .cyan, type: "log")
        }
    }

    // airportd answers the first BSSID request from a stale grant when the binary
    // changed, then withholds every later one. Fall back to the cache and ask again.
    private func downgradeToCacheSource() {
        bssidSource = .cache
        cacheReason = .noGrant
        canRecheckLiveSource = true
        liveSourceLinePrinted = false
        if onLiveSourceLost?() == true {
            writeEvent("Waiting for the Location prompt. Click Allow so iairport can read the SSID and BSSID.", color: .yellow, type: "log")
        } else {
            cacheWarningPrinted = false
            printCacheWarningIfNeeded()
        }
    }

    private func handleTransition(_ transition: AssociationTransition, sample: LinkSample, beforeIP: IPState?) {
        lastIPLine = nil
        switch transition.kind {
        case .roam:
            // A cache-mode `ROAM old -> ?` already counted this roam. When the
            // scan cache catches up, report the new BSSID without counting again.
            let resolvesCacheRoam = unresolvedCacheRoamFrom != nil && unresolvedCacheRoamFrom == transition.old?.bssid
            unresolvedCacheRoamFrom = nil
            if !resolvesCacheRoam { roams += 1 }
            driverSignal = nil
            flushPendingRoamCSVWithoutIP()
            if let beforeIP { pendingRoamIP = PendingRoamIP(before: beforeIP, latest: sample.ipState, start: transition.at, lastChange: transition.at) }
        case .reconnect:
            unresolvedCacheRoamFrom = nil
            reconnects += 1
        case .disconnect:
            unresolvedCacheRoamFrom = nil
            disconnects += 1
        case .join:
            unresolvedCacheRoamFrom = nil
        }
        updateHistory(transition: transition, sample: sample)
        let timing = transition.kind == .disconnect ? nil : timingForTransition(at: transition.at)
        let rendered = OutputFormatter.transition(transition, time: time)
        writeEvent(rendered.0, color: rendered.1, type: transition.kind.rawValue, json: OutputFormatter.transitionJSON(transition, time: time), bold: transition.kind == .roam)
        if transition.kind == .roam {
            if let flushed = pendingRoamCSV.replace(transition: transition, timing: timing) {
                csv?.writeTransition(flushed.transition, timing: flushed.timing)
            }
            Notifier.post(title: "iairport roam", message: OutputFormatter.roamNotification(transition), enabled: options.notify)
        } else {
            csv?.writeTransition(transition, timing: timing)
        }
        if let bssid = transition.new?.bssid {
            csv?.writeBSSID(bssid)
        }
        if transition.kind == .join || transition.kind == .reconnect {
            scheduleTransitionIPLine(sample.ipState, date: transition.at)
        }
    }

    private func updateHistory(transition: AssociationTransition, sample: LinkSample) {
        if let index = currentHistoryIndex, history.indices.contains(index) {
            history[index].rssiAtLeave = transition.old?.rssi
            history[index].leaveTime = transition.at
        }
        guard let new = transition.new else {
            currentHistoryIndex = nil
            return
        }
        let entry = RoamHistoryEntry(
            number: history.count + 1,
            joinTime: transition.at,
            ssid: new.ssid,
            bssid: new.bssid,
            apName: new.apName,
            vendor: sample.vendor,
            channel: new.channel,
            rssiAtJoin: new.rssi
        )
        history.append(entry)
        currentHistoryIndex = history.count - 1
    }

    private func handleIPObservation(_ ip: IPState, now: Date, transition: AssociationTransition?, beforeIP: IPState?) {
        if lastObservedIP == nil {
            lastObservedIP = ip
            return
        }
        if lastObservedIP != ip {
            lastObservedIP = ip
            if !ip.hasIPv4 && !ip.hasGlobalIPv6 && association.currentAssociation == nil {
                pendingIPEvent = nil
                ipLastChangedAt = nil
                return
            }
            if pendingTransitionIP != nil, ip.hasIPv4 || ip.hasGlobalIPv6 {
                writeIPLine(ip, date: now)
                pendingTransitionIP = nil
                pendingIPEvent = nil
                ipLastChangedAt = nil
                return
            }
            pendingIPEvent = ip
            ipLastChangedAt = now
            if transition == nil {
                queue.asyncAfter(deadline: .now() + .seconds(1)) { [weak self] in
                    self?.flushPendingIP(now: Date())
                }
            }
        }
    }

    private func flushPendingIP(now: Date) {
        guard let ip = pendingIPEvent, let changed = ipLastChangedAt else { return }
        guard now.timeIntervalSince(changed) >= 1.0 else { return }
        writeIPLine(ip, date: now)
        pendingIPEvent = nil
        ipLastChangedAt = nil
    }

    private func writeIPLine(_ ip: IPState, date: Date) {
        let line = ip.displayLine()
        // IPv6 temporary-address churn changes the state without changing the
        // text. Text output skips the repeat; JSON keeps every change.
        if !options.json, line == lastIPLine { return }
        lastIPLine = line
        writeEvent(line, color: .cyan, type: "ip", json: OutputFormatter.eventJSON(type: "ip", message: line, date: date, time: time, extra: ["ip": ip.jsonObject()]))
    }

    private func scheduleTransitionIPLine(_ ip: IPState, date: Date) {
        if ip.hasIPv4 || ip.hasGlobalIPv6 {
            writeIPLine(ip, date: date)
            return
        }
        pendingTransitionIP = PendingTransitionIP(deadline: date.addingTimeInterval(3.0), latest: ip)
        queue.asyncAfter(deadline: .now() + .seconds(3)) { [weak self] in
            self?.flushTransitionIP(now: Date())
        }
    }

    private func flushTransitionIP(now: Date) {
        guard let pending = pendingTransitionIP, now >= pending.deadline else { return }
        writeIPLine(lastObservedIP ?? pending.latest, date: now)
        pendingTransitionIP = nil
    }

    private func flushPendingRoamIP(now: Date) {
        guard var pending = pendingRoamIP else { return }
        if let observed = lastObservedIP, pending.latest != observed {
            pending.latest = observed
            pending.lastChange = now
            pendingRoamIP = pending
            return
        }
        guard now.timeIntervalSince(pending.lastChange) >= 1.0 else { return }
        let ms = Int(now.timeIntervalSince(pending.start) * 1000.0)
        let v4 = IPStateComparator.compareV4(before: pending.before, after: pending.latest, milliseconds: ms)
        let v6 = IPStateComparator.compareV6(before: pending.before, after: pending.latest, milliseconds: ms)
        let line = "IP after roam  \(v4.display(family: "v4"))  \(v6.display(family: "v6"))"
        writeEvent(line, color: .cyan, type: "ip", json: OutputFormatter.eventJSON(type: "ip", message: line, date: now, time: time))
        if let pendingCSV = pendingRoamCSV.take() {
            csv?.writeTransition(pendingCSV.transition, timing: pendingCSV.timing, v4: v4, v6: v6)
        }
        pendingRoamIP = nil
    }

    private func flushPendingRoamCSVWithoutIP() {
        if let pendingCSV = pendingRoamCSV.take() {
            csv?.writeTransition(pendingCSV.transition, timing: pendingCSV.timing)
        }
        pendingRoamIP = nil
    }

    private func handleLogLine(_ line: String) {
        if var capture = roamRequestCapture {
            switch capture.consume(line) {
            case .collecting:
                roamRequestCapture = capture
            case .finished(let info):
                roamRequestCapture = nil
                let eventLine = "ROAM REQUEST  target \(info.targetDisplay) ch \(info.channel.map(String.init) ?? "any") flags \(info.flags.map(String.init) ?? "")"
                var extra: [String: Any] = ["target": info.targetDisplay]
                if let channel = info.channel { extra["channel"] = channel }
                if let flags = info.flags { extra["flags"] = flags }
                writeEvent(eventLine, color: .magenta, type: "roam_request", json: OutputFormatter.eventJSON(type: "roam_request", message: eventLine, date: Date(), time: time, extra: extra))
            case .aborted(let reprocess):
                roamRequestCapture = nil
                handleLogLine(reprocess)
            }
            return
        }
        guard let event = LogClassifier.classify(line: line, verbose: options.verbose) else { return }
        switch event {
        case .driverRoamed:
            handleCacheModeDriverRoam()
            scheduleSettleReads(reason: "driver-roamed")
        case .linkSignal:
            scheduleSettleReads(reason: "link-signal")
        case .lqm(let lqm):
            apply(lqm: lqm)
        case .deauth(let kind, let source, let reason):
            let text = ReasonCodes.text(for: reason)
            let line = "\(time.status(Date()))  \(kind.uppercased())  reason \(reason) \(text)\(ReasonCodes.suffix(for: reason))\(source.map { " from \($0)" } ?? "")"
            writeEvent(line, color: .red, type: "deauth", json: OutputFormatter.eventJSON(type: "deauth", message: line, date: Date(), time: time, extra: ["reason": reason, "reason_text": text]))
        case .dhcp(let service):
            scheduleDHCP(service: service)
        case .dhcpv6(let service):
            scheduleDHCPv6(service: service)
        case .ipv6RA:
            let line = "IPv6 RA/SLAAC update"
            writeEvent(line, color: .cyan, type: "ip", json: OutputFormatter.eventJSON(type: "ip", message: line, date: Date(), time: time))
        case .joinTiming(let timing):
            let now = Date()
            guard joinTimingGate?.accept(timing, now: now) == true else { return }
            pendingJoinTiming = (timing, now)
            writeEvent(timing.line(), color: .cyan, type: "join_timing", json: OutputFormatter.eventJSON(type: "join_timing", message: timing.line(), date: now, time: time, extra: timing.values))
        case .associatedNetwork(let info):
            metrics.security = info.security
            metrics.ft = info.ft
            metrics.mfp = info.mfp
        case .roamRequestBegin:
            roamRequestCapture = RoamRequestCapture()
        case .scanSummary(let value):
            if options.verbose { writeEvent(value, color: .dim, type: "log") }
        case .roamLine(let value):
            writeEvent(value, color: .magenta, type: "log")
        case .problematic(let value):
            writeEvent(value, color: .red, type: "log")
        case .log(let value):
            writeEvent(value, color: .dim, type: "log")
        }
    }

    private func handleCacheModeDriverRoam() {
        guard bssidSource == .cache, let old = association.currentAssociation else { return }
        pendingCacheRoam = old
        queue.asyncAfter(deadline: .now() + .seconds(3)) { [weak self] in
            self?.flushPendingCacheRoam()
        }
    }

    private func flushPendingCacheRoam() {
        guard let old = pendingCacheRoam else { return }
        pendingCacheRoam = nil
        guard bssidSource == .cache,
              let transition = CacheModeRoam.transitionFromDriverSignal(old: old, cachedBSSIDAfterSettle: lastSample?.bssid, at: Date()) else { return }
        roams += 1
        driverSignal = nil
        unresolvedCacheRoamFrom = old.bssid
        let rendered = OutputFormatter.transition(transition, time: time)
        writeEvent(rendered.0, color: rendered.1, type: "roam", json: OutputFormatter.transitionJSON(transition, time: time), bold: true)
        csv?.writeTransition(transition, timing: nil)
    }

    private func apply(lqm: LQMMetrics) {
        metrics.ccaPct = lqm.cca
        metrics.snrDB = lqm.snr
        metrics.txRetrans = lqm.txRetrans
        metrics.txFail = lqm.txFail
        metrics.rxRetry = lqm.rxRetryFrames
        metrics.perAntennaRSSI = lqm.perAntennaRSSI
        if let band = lqm.band { metrics.band = band }
    }

    private func pollWdutil() {
        guard !wdutilInFlight else { return }
        wdutilInFlight = true
        let generation = association.generation
        WdutilInfo.runInfo(privilege: wdutilPrivilege) { [weak self] wd, ok in
            guard let self else { return }
            self.queue.async {
                self.wdutilInFlight = false
                if !ok {
                    self.wdutilPrivilege = .unavailable
                    self.wdutilTimer?.cancel()
                    self.wdutilTimer = nil
                    self.printWdutilHintOnce()
                    return
                }
                guard generation == self.association.generation, let wd else { return }
                self.metrics.mcs = wd.mcs
                self.metrics.nss = wd.nss
                self.metrics.guardIntervalNS = wd.guardIntervalNS
                if let cca = wd.cca { self.metrics.ccaPct = cca }
                if let phy = wd.phy { self.metrics.phy = phy }
                if let security = wd.security { self.metrics.security = security }
            }
        }
    }

    private func printWdutilHintOnce() {
        guard !wdutilHintPrinted else { return }
        wdutilHintPrinted = true
        renderer.event(line: "MCS/NSS/GI need root. Run `sudo -v` first or start with `sudo iairport`.", color: .yellow)
    }

    private func scheduleDHCP(service: String?) {
        dhcpService = service ?? dhcpService
        dhcpDeadline = Date().addingTimeInterval(2.0)
        queue.asyncAfter(deadline: .now() + .seconds(2)) { [weak self] in
            self?.flushDHCP(now: Date())
        }
    }

    private func scheduleDHCPv6(service: String?) {
        dhcpv6Service = service ?? dhcpv6Service
        dhcpv6Deadline = Date().addingTimeInterval(2.0)
        queue.asyncAfter(deadline: .now() + .seconds(2)) { [weak self] in
            self?.flushDHCP(now: Date())
        }
    }

    private func flushDHCP(now: Date) {
        if let deadline = dhcpDeadline, now >= deadline {
            let line = "DHCP changed\(dhcpService.map { " (service \($0))" } ?? "")"
            writeEvent(line, color: .cyan, type: "dhcp", json: OutputFormatter.eventJSON(type: "dhcp", message: line, date: now, time: time))
            dhcpDeadline = nil
            dhcpService = nil
        }
        if let deadline = dhcpv6Deadline, now >= deadline {
            let line = "DHCPv6 changed\(dhcpv6Service.map { " (service \($0))" } ?? "")"
            writeEvent(line, color: .cyan, type: "dhcp", json: OutputFormatter.eventJSON(type: "dhcp", message: line, date: now, time: time))
            dhcpv6Deadline = nil
            dhcpv6Service = nil
        }
    }

    private func checkDriverSignal() {
        guard let signal = driverSignal else { return }
        if signal.bssid == lastSample?.bssid {
            writeEvent("roam signal from driver, BSSID unchanged", color: .dim, type: "log")
        }
        driverSignal = nil
    }

    private func timingForTransition(at date: Date) -> JoinTiming? {
        guard let pending = pendingJoinTiming else { return nil }
        guard date.timeIntervalSince(pending.at) <= 10.0 else {
            pendingJoinTiming = nil
            return nil
        }
        pendingJoinTiming = nil
        return pending.timing
    }

    private func metricsFromLQMRate() -> Double? {
        nil
    }

    private func writeEvent(_ line: String, color: TextColor, type: String, json: [String: Any]? = nil, bold: Bool = false) {
        if options.json {
            renderer.json(json ?? OutputFormatter.eventJSON(type: type, message: line, date: Date(), time: time))
        } else {
            renderer.event(line: timestamped(line), color: color, bold: bold)
        }
    }

    private func timestamped(_ line: String) -> String {
        if line.range(of: "^\\d{4}/\\d{2}/\\d{2} ", options: .regularExpression) != nil {
            return line
        }
        return "\(time.status(Date()))  \(line)"
    }

    private func shutdown() {
        guard !shuttingDown else { return }
        shuttingDown = true
        timer?.cancel()
        wdutilTimer?.cancel()
        WdutilInfo.terminateChildren()
        coreWLANBridge?.stop()
        dynamicStore?.stop()
        logTail?.stop()
        flushPendingRoamCSVWithoutIP()
        csv?.close()
        printSummary()
        fflush(stdout)
        exit(0)
    }

    private func printSummary() {
        guard !options.json else {
            renderer.json(summaryJSON())
            return
        }
        let now = Date()
        renderer.clearStatus()
        renderer.event(line: "Summary", color: .white, bold: true, redrawStatus: false)
        renderer.event(line: "elapsed \(Units.elapsed(now.timeIntervalSince(startDate)))  roams \(roams)  reconnects \(reconnects)  disconnects \(disconnects)  distinct BSSIDs \(distinctBSSIDs.count)", redrawStatus: false)
        let bytes = ((lastSample?.bytesIn ?? 0) + (lastSample?.bytesOut ?? 0))
        renderer.event(line: "bytes \(Units.data(bytes))", redrawStatus: false)
        if let ip = lastSample?.ipState {
            renderer.event(line: "Final IPv4: \(ip.ipv4.joined(separator: " ")) gw \(ip.ipv4Router ?? "none")", redrawStatus: false)
            let v6 = ip.ipv6.map { item in
                let prefix = item.prefix.map { "/\($0)" } ?? ""
                return "\(item.address)\(prefix) \(item.kind.rawValue)"
            }.joined(separator: " ")
            renderer.event(line: "Final IPv6: \(v6) gw \(ip.ipv6Router ?? "none")", redrawStatus: false)
        }
        if !history.isEmpty {
            renderer.event(line: RoamHistoryEntry.header, redrawStatus: false)
            for var entry in history {
                // Names learned after the join still show in the table.
                if entry.apName == nil { entry.apName = apNames.name(for: entry.bssid) }
                renderer.event(line: entry.line(time: time, now: now), redrawStatus: false)
            }
        }
    }

    private func summaryJSON() -> [String: Any] {
        [
            "type": "summary",
            "ts": time.json(Date()),
            "roams": roams,
            "reconnects": reconnects,
            "disconnects": disconnects,
            "distinct_bssids": distinctBSSIDs.count
        ]
    }

    private func macOSLine() -> String {
        let process = Process()
        process.executableURL = URL(fileURLWithPath: "/usr/bin/sw_vers")
        process.arguments = []
        let pipe = Pipe()
        process.standardOutput = pipe
        process.standardError = Pipe()
        if (try? process.run()) != nil {
            let data = pipe.fileHandleForReading.readDataToEndOfFile()
            process.waitUntilExit()
            if let text = String(data: data, encoding: .utf8) {
                var version = ""
                var build = ""
                for line in text.split(separator: "\n") {
                    if line.hasPrefix("ProductVersion:") { version = line.split(separator: "\t").last.map(String.init) ?? "" }
                    if line.hasPrefix("BuildVersion:") { build = line.split(separator: "\t").last.map(String.init) ?? "" }
                }
                return "macOS \(version) build \(build)"
            }
        }
        let os = ProcessInfo.processInfo.operatingSystemVersion
        return "macOS \(os.majorVersion).\(os.minorVersion).\(os.patchVersion)"
    }

    private func serviceID(from key: String?) -> String? {
        guard let key else { return nil }
        let parts = key.split(separator: "/")
        guard let serviceIndex = parts.firstIndex(of: "Service"), parts.index(after: serviceIndex) < parts.endIndex else { return nil }
        return String(parts[parts.index(after: serviceIndex)])
    }
}

private struct PendingRoamIP {
    var before: IPState
    var latest: IPState
    var start: Date
    var lastChange: Date
}

private struct PendingTransitionIP {
    var deadline: Date
    var latest: IPState
}

private struct DriverSignal {
    var bssid: String?
    var at: Date
}

public enum SignalInstaller {
    private static var sources: [DispatchSourceSignal] = []
    private static let countLock = NSLock()
    private static var count = 0

    /// Counts delivered shutdown signals and returns the new total.
    public static func recordSignal() -> Int {
        countLock.lock()
        defer { countLock.unlock() }
        count += 1
        return count
    }

    public static func install(on queue: DispatchQueue = DispatchQueue.global(), handler: @escaping () -> Void) {
        signal(SIGPIPE, SIG_IGN)
        for sig in [SIGINT, SIGTERM, SIGHUP] {
            signal(sig, SIG_IGN)
            let source = DispatchSource.makeSignalSource(signal: sig, queue: queue)
            source.setEventHandler(handler: handler)
            source.resume()
            sources.append(source)
        }
    }
}
