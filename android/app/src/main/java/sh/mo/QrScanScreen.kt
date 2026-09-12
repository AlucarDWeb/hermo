package sh.mo

import android.content.Context
import androidx.camera.core.CameraSelector
import androidx.camera.core.ImageAnalysis
import androidx.camera.core.ImageProxy
import androidx.camera.core.Preview
import androidx.camera.lifecycle.ProcessCameraProvider
import androidx.camera.view.PreviewView
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.height
import androidx.compose.foundation.layout.padding
import androidx.compose.material3.Button
import androidx.compose.material3.Text
import androidx.compose.runtime.Composable
import androidx.compose.runtime.DisposableEffect
import androidx.compose.runtime.remember
import androidx.compose.ui.Modifier
import androidx.compose.ui.platform.LocalContext
import androidx.compose.ui.platform.LocalLifecycleOwner
import androidx.compose.ui.unit.dp
import androidx.compose.ui.viewinterop.AndroidView
import androidx.core.content.ContextCompat
import com.google.zxing.BinaryBitmap
import com.google.zxing.DecodeHintType
import com.google.zxing.MultiFormatReader
import com.google.zxing.PlanarYUVLuminanceSource
import com.google.zxing.common.HybridBinarizer
import java.util.concurrent.ExecutorService
import java.util.concurrent.Executors

/**
 * QR scan screen, NO Google Play Services (T6b): CameraX (AndroidX) preview
 * + `ImageAnalysis` frames fed to ZXing's `MultiFormatReader` on a background
 * executor. Functional PoC: preview, decode, one line of state. When a
 * `hermes://connect?...` payload decodes, [onDecoded] fires once.
 */
@Composable
fun QrScanScreen(onDecoded: (String) -> Unit, onCancel: () -> Unit) {
    val context = LocalContext.current
    val lifecycleOwner = LocalLifecycleOwner.current
    val executor = remember { Executors.newSingleThreadExecutor() }
    val fired = remember { androidx.compose.runtime.mutableStateOf(false) }

    DisposableEffect(Unit) {
        onDispose { executor.shutdown() }
    }

    Column(modifier = Modifier.fillMaxSize()) {
        Text(
            text = "Point the camera at the gateway's QR",
            modifier = Modifier.padding(8.dp),
        )
        AndroidView(
            factory = { ctx ->
                PreviewView(ctx).also { previewView ->
                    startCamera(ctx, lifecycleOwner, previewView, executor) { payload ->
                        // Fire exactly once, on the main thread's recompose.
                        if (!fired.value) {
                            fired.value = true
                            previewView.post { onDecoded(payload) }
                        }
                    }
                }
            },
            modifier = Modifier
                .fillMaxWidth()
                .height(320.dp),
        )
        Button(onClick = onCancel, modifier = Modifier.padding(8.dp)) {
            Text("Cancel")
        }
    }
}

private fun startCamera(
    context: Context,
    lifecycleOwner: androidx.lifecycle.LifecycleOwner,
    previewView: PreviewView,
    executor: ExecutorService,
    onDecoded: (String) -> Unit,
) {
    val providerFuture = ProcessCameraProvider.getInstance(context)
    providerFuture.addListener({
        try {
            val provider = providerFuture.get()
            val preview = Preview.Builder().build().also {
                it.setSurfaceProvider(previewView.surfaceProvider)
            }
            val reader = MultiFormatReader().apply {
                setHints(mapOf(DecodeHintType.POSSIBLE_FORMATS to listOf(com.google.zxing.BarcodeFormat.QR_CODE)))
            }
            val analysis = ImageAnalysis.Builder()
                .setBackpressureStrategy(ImageAnalysis.STRATEGY_KEEP_ONLY_LATEST)
                .build()
            analysis.setAnalyzer(executor) { frame ->
                decodeFrame(reader, frame)?.let(onDecoded)
                frame.close()
            }
            provider.unbindAll()
            // The emulator may expose only a front camera (and `-camera-back
            // none` none at all): pick whichever exists instead of assuming
            // DEFAULT_BACK_CAMERA. A real phone has the back camera.
            val selector = availableCameraSelector(provider)
                ?: CameraSelector.DEFAULT_BACK_CAMERA
            provider.bindToLifecycle(lifecycleOwner, selector, preview, analysis)
        } catch (_: Exception) {
            // Camera init failure (emulator without camera, permission race):
            // the cancel button stays available; no crash (functional bar).
        }
    }, ContextCompat.getMainExecutor(context))
}

/**
 * A selector for whichever camera the device actually exposes (front on
 * many emulator configs, back on real phones). `DEFAULT_BACK_CAMERA` on a
 * front-only device throws in `bindToLifecycle` ("No available camera can
 * be found" — observed on the API 36 emulator), which would leave the scan
 * screen dead without any error.
 */
private fun availableCameraSelector(provider: ProcessCameraProvider): CameraSelector? =
    if (provider.hasCamera(CameraSelector.DEFAULT_BACK_CAMERA)) {
        CameraSelector.DEFAULT_BACK_CAMERA
    } else if (provider.hasCamera(CameraSelector.DEFAULT_FRONT_CAMERA)) {
        CameraSelector.DEFAULT_FRONT_CAMERA
    } else {
        null
    }

/** YUV plane 0 -> luminance source -> ZXing. Returns null when nothing decoded. */
private fun decodeFrame(reader: MultiFormatReader, frame: ImageProxy): String? {
    return try {
        val plane = frame.planes[0]
        val bytes = plane.buffer
        val data = ByteArray(bytes.remaining())
        bytes.get(data)
        val source = PlanarYUVLuminanceSource(
            data, plane.rowStride, frame.height, 0, 0, frame.width.coerceAtMost(plane.rowStride), frame.height, false,
        )
        val result = reader.decodeWithState(BinaryBitmap(HybridBinarizer(source)))
        result.text
    } catch (_: Exception) {
        null
    } finally {
        reader.reset()
    }
}
