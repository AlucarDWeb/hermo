import AVFoundation
import SwiftUI

/// Camera scanner for the gateway's pairing QR (`QrScanScreen.kt`): a 320 pt `AVCaptureSession`
/// preview restricted to `.qr`, firing `onDecoded` once. `onDecoded` and `onPermissionDenied`
/// are `@Sendable` because they are called from the capture session's background actor, not
/// from the view's own context.
public struct QrScanView: View {
    private let onDecoded: @Sendable (String) -> Void
    private let onCancel: () -> Void
    private let onPermissionDenied: @Sendable () -> Void

    @Environment(\.hermoTokens) private var tokens

    public init(
        onDecoded: @escaping @Sendable (String) -> Void,
        onCancel: @escaping () -> Void,
        onPermissionDenied: @escaping @Sendable () -> Void
    ) {
        self.onDecoded = onDecoded
        self.onCancel = onCancel
        self.onPermissionDenied = onPermissionDenied
    }

    public var body: some View {
        VStack(spacing: 0) {
            Text("Point the camera at the gateway's QR")
                .font(HermoFonts.bodyMedium)
                .foregroundStyle(tokens.text)
                .padding(8)
            QrCaptureRepresentable(onDecoded: onDecoded, onPermissionDenied: onPermissionDenied)
                .frame(maxWidth: .infinity)
                .frame(height: 320)
                .accessibilityIdentifier("hermo.qrScan.preview")
            Button("Cancel", action: onCancel)
                .buttonStyle(.bordered)
                .tint(tokens.primary)
                .padding(8)
                .accessibilityIdentifier("hermo.qrScan.cancel")
        }
        .frame(maxWidth: .infinity, maxHeight: .infinity)
        .background(tokens.background.ignoresSafeArea())
    }
}

/// Bridges the `AVCaptureVideoPreviewLayer` into SwiftUI; the session lifecycle lives in the coordinator.
private struct QrCaptureRepresentable: UIViewRepresentable {
    let onDecoded: @Sendable (String) -> Void
    let onPermissionDenied: @Sendable () -> Void

    init(onDecoded: @escaping @Sendable (String) -> Void, onPermissionDenied: @escaping @Sendable () -> Void) {
        self.onDecoded = onDecoded
        self.onPermissionDenied = onPermissionDenied
    }

    func makeUIView(context: Context) -> QrPreviewView {
        let view = QrPreviewView()
        context.coordinator.attach(to: view)
        return view
    }

    func updateUIView(_ uiView: QrPreviewView, context: Context) {}

    func makeCoordinator() -> QrScanCoordinator {
        QrScanCoordinator(onDecoded: onDecoded, onPermissionDenied: onPermissionDenied)
    }

    static func dismantleUIView(_ uiView: QrPreviewView, coordinator: QrScanCoordinator) {
        coordinator.detach()
    }
}

/// Hosts the `AVCaptureVideoPreviewLayer` and keeps its frame in sync with layout.
private final class QrPreviewView: UIView {
    private var previewLayer: AVCaptureVideoPreviewLayer?

    func setPreviewLayer(_ newLayer: AVCaptureVideoPreviewLayer) {
        previewLayer?.removeFromSuperlayer()
        newLayer.frame = bounds
        layer.addSublayer(newLayer)
        previewLayer = newLayer
    }

    override func layoutSubviews() {
        super.layoutSubviews()
        previewLayer?.frame = bounds
    }
}

/// Owns permission request and session attach/detach, both asked for from the main actor but
/// carried out on `QrCaptureSession`'s own actor so `startRunning`/`stopRunning` never block the UI.
@MainActor
private final class QrScanCoordinator {
    private let onDecoded: @Sendable (String) -> Void
    private let onPermissionDenied: @Sendable () -> Void
    private var captureSession: QrCaptureSession?
    private var isDetached = false

    init(onDecoded: @escaping @Sendable (String) -> Void, onPermissionDenied: @escaping @Sendable () -> Void) {
        self.onDecoded = onDecoded
        self.onPermissionDenied = onPermissionDenied
    }

    func attach(to view: QrPreviewView) {
        let onDecoded = onDecoded
        let onPermissionDenied = onPermissionDenied
        Task {
            let granted = await AVCaptureDevice.requestAccess(for: .video)
            guard granted else {
                onPermissionDenied()
                return
            }
            guard !isDetached else { return }
            let session = QrCaptureSession { payload in
                Task { @MainActor in onDecoded(payload) }
            }
            captureSession = session
            let previewLayer = await session.makePreviewLayer()
            guard !isDetached else {
                await session.stop()
                return
            }
            view.setPreviewLayer(previewLayer)
            await session.start()
        }
    }

    func detach() {
        isDetached = true
        guard let captureSession else { return }
        self.captureSession = nil
        Task { await captureSession.stop() }
    }
}

/// Configures and runs the capture session off the main actor; `hasFired` makes the one-shot
/// decode atomic without a lock, since actor isolation already serializes access to it.
private actor QrCaptureSession {
    private let session = AVCaptureSession()
    private let delegateQueue = DispatchQueue(label: "sh.mo.hermo.qrscan")
    private var delegate: QrScanMetadataDelegate?
    private var hasFired = false
    private var isStopped = false
    private let onDecoded: @Sendable (String) -> Void

    init(onDecoded: @escaping @Sendable (String) -> Void) {
        self.onDecoded = onDecoded
    }

    func makePreviewLayer() -> AVCaptureVideoPreviewLayer {
        let previewLayer = AVCaptureVideoPreviewLayer(session: session)
        previewLayer.videoGravity = .resizeAspectFill
        return previewLayer
    }

    func start() {
        // `stop` can land on the actor first when the sheet is dismissed mid-attach, and a
        // start after it would leave the camera running with nothing on screen.
        guard !isStopped else { return }
        guard
            let device = AVCaptureDevice.default(for: .video),
            let input = try? AVCaptureDeviceInput(device: device),
            session.canAddInput(input)
        else { return }
        session.addInput(input)

        let output = AVCaptureMetadataOutput()
        guard session.canAddOutput(output) else { return }
        session.addOutput(output)

        let delegate = QrScanMetadataDelegate { [weak self] payload in
            Task { await self?.handleDecoded(payload) }
        }
        self.delegate = delegate
        output.setMetadataObjectsDelegate(delegate, queue: delegateQueue)
        // `availableMetadataObjectTypes` is only populated once the output is on the session.
        guard output.availableMetadataObjectTypes.contains(.qr) else { return }
        output.metadataObjectTypes = [.qr]

        session.startRunning()
    }

    func stop() {
        isStopped = true
        if session.isRunning {
            session.stopRunning()
        }
    }

    private func handleDecoded(_ payload: String) {
        guard !hasFired else { return }
        hasFired = true
        if session.isRunning {
            session.stopRunning()
        }
        onDecoded(payload)
    }
}

private final class QrScanMetadataDelegate: NSObject, AVCaptureMetadataOutputObjectsDelegate {
    private let onPayload: @Sendable (String) -> Void

    init(onPayload: @escaping @Sendable (String) -> Void) {
        self.onPayload = onPayload
    }

    func metadataOutput(
        _ output: AVCaptureMetadataOutput,
        didOutput metadataObjects: [AVMetadataObject],
        from connection: AVCaptureConnection
    ) {
        guard
            let code = metadataObjects.first as? AVMetadataMachineReadableCodeObject,
            code.type == .qr,
            let payload = code.stringValue
        else { return }
        onPayload(payload)
    }
}

#Preview("Light") {
    QrScanView(onDecoded: { _ in }, onCancel: {}, onPermissionDenied: {})
        .hermoTheme(.light)
}

#Preview("Dark") {
    QrScanView(onDecoded: { _ in }, onCancel: {}, onPermissionDenied: {})
        .hermoTheme(.dark)
}
