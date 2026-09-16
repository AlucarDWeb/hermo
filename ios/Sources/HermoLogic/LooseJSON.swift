import Foundation

/// A JSON tree that never throws: every accessor falls back to a supplied default
/// on a missing key, a JSON null, or a value of the wrong shape. Mirrors org.json's
/// `opt*` family, which the Android app relies on for gateway JSON it does not control.
public struct LooseJSON: Sendable {
    private let root: LooseJSONValue

    /// Returns nil only when `text` fails to parse as JSON at all; a value of the
    /// wrong shape (a bare number, a string) still parses, it just answers every
    /// keyed lookup with the caller's default.
    public init?(_ text: String) {
        guard let data = text.data(using: .utf8),
              let parsed = try? JSONSerialization.jsonObject(with: data, options: [.fragmentsAllowed])
        else { return nil }
        root = Self.convert(parsed)
    }

    /// Wraps a value already produced by `JSONSerialization`, or a plain Swift
    /// literal built for a test fixture.
    public init(_ value: Any?) {
        root = value.map(Self.convert) ?? .null
    }

    private init(root: LooseJSONValue) {
        self.root = root
    }

    private func field(_ key: String) -> LooseJSONValue? {
        guard case .object(let fields) = root else { return nil }
        // A later duplicate key wins, matching how org.json's own map overwrites on parse.
        return fields.last(where: { $0.key == key })?.value
    }

    /// Present and not JSON null, org.json's `has(key) && !isNull(key)`. The
    /// callers that need it read fields where `0` and absence mean different
    /// things, which no `opt*` default can express.
    public func has(_ key: String) -> Bool {
        guard let value = field(key) else { return false }
        if case .null = value { return false }
        return true
    }

    /// Whether this node is a JSON object. `JSONObject(text)` throws on anything
    /// else, so a caller that mirrors it has to reject a bare array or scalar.
    public var isObject: Bool {
        if case .object = root { return true }
        return false
    }

    public func optString(_ key: String, default def: String = "") -> String {
        guard let value = field(key) else { return def }
        return Self.stringCoercion(value) ?? def
    }

    public func optInt(_ key: String, default def: Int = 0) -> Int {
        guard let value = field(key), let n = Self.numberCoercion(value) else { return def }
        return Int(clamping: Self.saturatingInt64(n))
    }

    public func optLong(_ key: String, default def: Int64 = 0) -> Int64 {
        guard let value = field(key), let n = Self.numberCoercion(value) else { return def }
        return Self.saturatingInt64(n)
    }

    public func optDouble(_ key: String, default def: Double = 0) -> Double {
        guard let value = field(key), let n = Self.numberCoercion(value) else { return def }
        return n
    }

    public func optBool(_ key: String, default def: Bool = false) -> Bool {
        guard let value = field(key) else { return def }
        switch value {
        case .bool(let b):
            return b
        case .string(let s):
            if s.caseInsensitiveCompare("true") == .orderedSame { return true }
            if s.caseInsensitiveCompare("false") == .orderedSame { return false }
            return def
        default:
            return def
        }
    }

    public func optObject(_ key: String) -> LooseJSON? {
        guard let value = field(key), case .object = value else { return nil }
        return LooseJSON(root: value)
    }

    /// An absent key, a null, or a value that is not a JSON array all yield an
    /// empty array, never nil: a caller loops over the result unconditionally.
    public func optArray(_ key: String) -> [LooseJSON] {
        guard let value = field(key), case .array(let items) = value else { return [] }
        return items.map { LooseJSON(root: $0) }
    }

    /// Reads this node itself as a string, for elements of an array of bare
    /// strings (there is no key to look under, unlike `optString(_:default:)`).
    public func stringValue(default def: String = "") -> String {
        Self.stringCoercion(root) ?? def
    }

    /// `JSONObject.toString(2)`'s twin: two-space indent per level, an empty
    /// object or array collapsed onto one line. Never throws; a node that is
    /// itself a bare scalar just renders as that scalar.
    public func pretty(indent: Int = 2) -> String {
        Self.render(root, indent: indent, level: 0)
    }

    /// One line, no spaces, the shape `JSONArray.toString()` and
    /// `JSONObject.toString()` produce.
    public func compact() -> String {
        Self.renderCompact(root)
    }

    private static func stringCoercion(_ value: LooseJSONValue) -> String? {
        switch value {
        case .string(let s): return s
        case .bool(let b): return b ? "true" : "false"
        case .number(let n): return numberText(n)
        // org.json hands back `toString()` for a nested object or array, which is
        // how `args`, `result` and `usage` cross as text whatever their shape.
        case .object, .array: return renderCompact(value)
        case .null: return nil
        }
    }

    private static func numberCoercion(_ value: LooseJSONValue) -> Double? {
        switch value {
        case .number(let n): return n
        case .string(let s): return Double(s)
        case .null, .bool, .object, .array: return nil
        }
    }

    /// Java's narrowing conversion, which `org.json`'s `optInt`/`optLong` inherit:
    /// NaN becomes 0 and an out-of-range magnitude saturates. Swift's own
    /// `Int64(Double)` traps on both.
    private static func saturatingInt64(_ n: Double) -> Int64 {
        if n.isNaN { return 0 }
        if n >= 0x1p63 { return .max }
        if n < -0x1p63 { return .min }
        return Int64(n)
    }

    private static func numberText(_ n: Double) -> String {
        if n.isFinite && n == n.rounded() && abs(n) < 1e15 {
            return String(Int64(n))
        }
        return String(n)
    }

    private static func render(_ value: LooseJSONValue, indent: Int, level: Int) -> String {
        switch value {
        case .null:
            return "null"
        case .bool(let b):
            return b ? "true" : "false"
        case .number(let n):
            return numberText(n)
        case .string(let s):
            return encodeString(s)
        case .array(let items):
            if items.isEmpty { return "[]" }
            let pad = String(repeating: " ", count: indent * (level + 1))
            let closePad = String(repeating: " ", count: indent * level)
            let body = items
                .map { pad + render($0, indent: indent, level: level + 1) }
                .joined(separator: ",\n")
            return "[\n\(body)\n\(closePad)]"
        case .object(let fields):
            if fields.isEmpty { return "{}" }
            let pad = String(repeating: " ", count: indent * (level + 1))
            let closePad = String(repeating: " ", count: indent * level)
            let body = fields
                .map { pad + encodeString($0.key) + ": " + render($0.value, indent: indent, level: level + 1) }
                .joined(separator: ",\n")
            return "{\n\(body)\n\(closePad)}"
        }
    }

    private static func renderCompact(_ value: LooseJSONValue) -> String {
        switch value {
        case .null:
            return "null"
        case .bool(let b):
            return b ? "true" : "false"
        case .number(let n):
            return numberText(n)
        case .string(let s):
            return encodeString(s)
        case .array(let items):
            return "[" + items.map(renderCompact).joined(separator: ",") + "]"
        case .object(let fields):
            let body = fields
                .map { encodeString($0.key) + ":" + renderCompact($0.value) }
                .joined(separator: ",")
            return "{" + body + "}"
        }
    }

    private static func encodeString(_ s: String) -> String {
        var out = "\""
        for scalar in s.unicodeScalars {
            switch scalar {
            case "\"": out += "\\\""
            case "\\": out += "\\\\"
            case "\n": out += "\\n"
            case "\r": out += "\\r"
            case "\t": out += "\\t"
            default:
                if scalar.value < 0x20 {
                    out += String(format: "\\u%04x", scalar.value)
                } else {
                    out.unicodeScalars.append(scalar)
                }
            }
        }
        out += "\""
        return out
    }

    private static func convert(_ any: Any) -> LooseJSONValue {
        if any is NSNull { return .null }
        if let number = any as? NSNumber {
            if CFGetTypeID(number) == CFBooleanGetTypeID() {
                return .bool(number.boolValue)
            }
            return .number(number.doubleValue)
        }
        if let string = any as? String { return .string(string) }
        if let array = any as? [Any] { return .array(array.map(convert)) }
        if let dict = any as? [String: Any] {
            // NSDictionary's own key order, when the value came straight out of
            // JSONSerialization, tends to follow the source text on this platform;
            // it is not a documented guarantee, so this is best effort only.
            let keys = (any as? NSDictionary)?.allKeys as? [String] ?? Array(dict.keys)
            let fields = keys.compactMap { key in
                dict[key].map { LooseJSONField(key: key, value: convert($0)) }
            }
            return .object(fields)
        }
        return .null
    }
}

private struct LooseJSONField: Sendable {
    let key: String
    let value: LooseJSONValue
}

private enum LooseJSONValue: Sendable {
    case null
    case string(String)
    case number(Double)
    case bool(Bool)
    case array([LooseJSONValue])
    case object([LooseJSONField])
}
