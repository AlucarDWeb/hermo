public struct TabSet: Equatable, Sendable {
    public var keys: [String]
    public var current: String?

    public init(keys: [String] = [], current: String? = nil) {
        self.keys = keys
        self.current = current
    }
}

public struct RestorePlan: Equatable, Sendable {
    public let resumeKeys: [String]
    public let current: String?

    public init(resumeKeys: [String], current: String?) {
        self.resumeKeys = resumeKeys
        self.current = current
    }
}

extension TabSet {
    public func select(_ key: String) -> TabSet {
        keys.contains(key) ? TabSet(keys: keys, current: key) : self
    }

    /// A key already open only becomes current; re-adding it must not grow the list or trigger a re-open that resets the transcript.
    public func add(_ key: String) -> TabSet {
        if keys.contains(key) {
            return TabSet(keys: keys, current: key)
        }
        return TabSet(keys: keys + [key], current: key)
    }

    /// Closing the last remaining tab empties the set instead of refusing the close; the caller mints a fresh chat afterward.
    public func close(_ key: String) -> TabSet {
        guard let index = keys.firstIndex(of: key) else { return self }
        var remaining = keys
        remaining.remove(at: index)
        let nextCurrent: String?
        if current != key {
            nextCurrent = current
        } else if index > 0 {
            nextCurrent = remaining[index - 1]
        } else {
            nextCurrent = remaining.first
        }
        return TabSet(keys: remaining, current: nextCurrent)
    }
}

public func restorePlan(keys: [String], lastActive: String?) -> RestorePlan {
    let current = (lastActive.flatMap { keys.contains($0) ? $0 : nil }) ?? keys.last
    return RestorePlan(resumeKeys: keys, current: current)
}

public enum LaunchStep: Equatable, Sendable {
    case listFailed
    case runPlan(RestorePlan)
}

/// `registryKeys` nil means the registry read failed; only a successful empty list is the genuine fresh-install case.
public func launchStep(registryKeys: [String]?, lastActive: String?) -> LaunchStep {
    guard let registryKeys else { return .listFailed }
    return .runPlan(restorePlan(keys: registryKeys, lastActive: lastActive))
}

/// Tapping the already-current tab returns nil so the caller does not re-issue a header refresh.
public func tabTap(tabs: TabSet, currentKey: String?, key: String) -> TabSet? {
    if key == currentKey || !tabs.keys.contains(key) { return nil }
    return tabs.select(key)
}
