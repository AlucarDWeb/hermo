import Foundation
import HermesCore

public enum ErrorMessages {

    public static func of(_ error: Error) -> String {
        if let coreError = error as? CoreError {
            switch coreError {
            case .SessionExpired:
                return "Session expired — enter the password again."
            case .InvalidCredentials:
                return "Wrong username or password."
            case .UpgradeRejected:
                return "The gateway rejected the connection (host/origin guard)."
            case .RateLimited:
                return "Too many attempts — wait a moment and retry."
            case .NotConnected:
                return "Not connected to the gateway yet."
            case .InvalidQr:
                return "That pairing payload is not valid."
            case .InvalidEndpoint:
                return "That gateway URL is not valid."
            case .Timeout:
                return "The gateway did not answer in time."
            case .Network:
                return "Network error — check the connection."
            case .UnknownProvider:
                return "The gateway has no password auth enabled."
            case .Rpc(let code, let detail):
                return "Error: rpc error \(code): \(detail)"
            case .Io(let detail):
                return "Error: io error: \(detail)"
            case .UnexpectedStatus:
                return "Error: unexpected submit status"
            case .Http(let status):
                return "Error: http status \(status)"
            }
        }

        let message = (error as? LocalizedError)?.errorDescription ?? String(describing: error)
        if message.contains("invalid QR payload") {
            return "That pairing payload is not valid."
        }
        if message.contains("invalid endpoint url") {
            return "That gateway URL is not valid."
        }
        if message.contains("invalid credentials") {
            return "Wrong username or password."
        }
        if message.contains("session expired") {
            return "Session expired — enter the password again."
        }
        if message.contains("rate limited") {
            return "Too many attempts — wait a moment and retry."
        }
        if message.contains("upgrade rejected") {
            return "The gateway rejected the connection (host/origin guard)."
        }
        if message.contains("not connected") {
            return "Not connected to the gateway yet."
        }
        if message.contains("operation timed out") {
            return "The gateway did not answer in time."
        }
        if message.contains("network failure") {
            return "Network error — check the connection."
        }
        if message.contains("unknown auth provider") {
            return "The gateway has no password auth enabled."
        }
        return "Error: \(message)"
    }

    public static func isAuthShape(_ error: Error) -> Bool {
        guard let coreError = error as? CoreError else { return false }
        switch coreError {
        case .SessionExpired, .InvalidCredentials, .UpgradeRejected:
            return true
        default:
            return false
        }
    }
}
