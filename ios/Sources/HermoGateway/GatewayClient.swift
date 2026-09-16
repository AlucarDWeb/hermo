import ComposableArchitecture
import Foundation
import HermesCore

@DependencyClient
public struct GatewayClient: Sendable {
    public var savedEndpoint: @Sendable () async -> EndpointDto?
    public var pair: @Sendable (_ payload: String) async throws -> EndpointDto
    public var login: @Sendable (_ password: String) async throws -> Void
    public var connect: @Sendable () async throws -> Void
    public var openSessions: @Sendable () async -> [SessionSummary] = { [] }
    public var lastActiveSession: @Sendable () async -> String?
    public var openSession: @Sendable (_ storedId: String?, _ cols: Int64) async throws -> String
    public var openBotChat: @Sendable (_ profile: String, _ cols: Int64) async throws -> String
    public var listProfiles: @Sendable () async throws -> [ProfileSummaryDto]
    public var closeSession: @Sendable (_ key: String) async throws -> Void
    public var setSessionTitle: @Sendable (_ key: String, _ title: String) async throws -> Void
    public var clearSessions: @Sendable () async throws -> Void
    public var forgetGateway: @Sendable () async throws -> Void
    public var send: @Sendable (_ key: String, _ text: String) async throws -> Void
    public var interrupt: @Sendable (_ key: String) async throws -> Void
    public var respondApproval: @Sendable (_ key: String, _ requestId: String, _ choice: String) async throws -> Void
    public var respondClarify: @Sendable (
        _ key: String, _ requestId: String, _ answer: String, _ questionId: String?
    ) async throws -> Void
    public var runSlash: @Sendable (_ key: String, _ command: String) async throws -> SlashOutcome
    public var completeSlash: @Sendable (_ text: String) async throws -> SlashCompletionsDto
    public var listRemoteSessions: @Sendable () async throws -> [RemoteSessionDto]
    public var appDidForeground: @Sendable () async throws -> Void
    public var disconnect: @Sendable () async throws -> Void
    public var events: @Sendable () -> AsyncStream<CoreEvent> = { AsyncStream { $0.finish() } }
}

/// Owns the one `HermesCore` and `EventSinkBridge` the app ever builds, as `static let`
/// globals that live for the process: the generated `Drop` moves the tokio runtime to a
/// detached thread when the core is deallocated, which must never happen.
private enum GatewayRuntime {
    static let core = HermesCore(dataDir: dataDirectory().path)
    static let bridge = EventSinkBridge()

    /// Caches is eviction-eligible under storage pressure and would lose the cookie jar and
    /// the tab registry, so the core's files live under Application Support instead.
    private static func dataDirectory() -> URL {
        let fileManager = FileManager.default
        guard
            let base = try? fileManager.url(
                for: .applicationSupportDirectory,
                in: .userDomainMask,
                appropriateFor: nil,
                create: true
            )
        else {
            fatalError("Application Support directory is unavailable.")
        }
        let directory = base.appendingPathComponent("hermo", isDirectory: true)
        if !fileManager.fileExists(atPath: directory.path) {
            try? fileManager.createDirectory(at: directory, withIntermediateDirectories: true)
        }
        return directory
    }
}

extension GatewayClient: TestDependencyKey {
    public static let testValue = Self()
}

extension GatewayClient: DependencyKey {
    public static let liveValue = Self(
        savedEndpoint: { await GatewayRuntime.core.savedEndpoint() },
        pair: { payload in try await GatewayRuntime.core.pair(payload: payload) },
        login: { password in try await GatewayRuntime.core.login(password: password) },
        connect: { try await GatewayRuntime.core.connect(sink: GatewayRuntime.bridge) },
        openSessions: { await GatewayRuntime.core.openSessions() },
        lastActiveSession: { await GatewayRuntime.core.lastActiveSession() },
        openSession: { storedId, cols in
            try await GatewayRuntime.core.openSession(storedId: storedId, cols: cols)
        },
        openBotChat: { profile, cols in
            try await GatewayRuntime.core.openBotChat(profile: profile, cols: cols)
        },
        listProfiles: { try await GatewayRuntime.core.listProfiles() },
        closeSession: { key in try await GatewayRuntime.core.closeSession(key: key) },
        setSessionTitle: { key, title in
            try await GatewayRuntime.core.setSessionTitle(key: key, title: title)
        },
        clearSessions: { try await GatewayRuntime.core.clearSessions() },
        forgetGateway: { try await GatewayRuntime.core.forgetGateway() },
        send: { key, text in try await GatewayRuntime.core.send(key: key, text: text) },
        interrupt: { key in try await GatewayRuntime.core.interrupt(key: key) },
        respondApproval: { key, requestId, choice in
            try await GatewayRuntime.core.respondApproval(key: key, requestId: requestId, choice: choice)
        },
        respondClarify: { key, requestId, answer, questionId in
            try await GatewayRuntime.core.respondClarify(
                key: key, requestId: requestId, answer: answer, questionId: questionId
            )
        },
        runSlash: { key, command in try await GatewayRuntime.core.runSlash(key: key, command: command) },
        completeSlash: { text in try await GatewayRuntime.core.completeSlash(text: text) },
        listRemoteSessions: { try await GatewayRuntime.core.listRemoteSessions() },
        appDidForeground: { try await GatewayRuntime.core.appDidForeground() },
        disconnect: { try await GatewayRuntime.core.disconnect() },
        events: { GatewayRuntime.bridge.events }
    )
}

extension DependencyValues {
    public var gatewayClient: GatewayClient {
        get { self[GatewayClient.self] }
        set { self[GatewayClient.self] = newValue }
    }
}
