import HermesCore

extension EndpointDto {
    public var displayText: String {
        "\(displayName) (\(username)) — \(baseUrl)"
    }
}
