package sh.mo

import android.Manifest
import android.content.pm.PackageManager
import android.os.Bundle
import androidx.activity.ComponentActivity
import androidx.activity.compose.setContent
import androidx.activity.result.contract.ActivityResultContracts
import androidx.activity.viewModels
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.padding
import androidx.compose.material3.Button
import androidx.compose.material3.LinearProgressIndicator
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.OutlinedButton
import androidx.compose.material3.OutlinedTextField
import androidx.compose.material3.Surface
import androidx.compose.material3.Text
import androidx.compose.runtime.Composable
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.runtime.collectAsState
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.setValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.platform.LocalDensity
import androidx.compose.ui.platform.LocalView
import androidx.compose.ui.unit.dp
import androidx.core.content.ContextCompat
import androidx.lifecycle.viewmodel.compose.viewModel

/**
 * Single-activity host (T6b): renders the screen for the current [AppPhase].
 * The phase comes from the view model's flows — the composable never infers
 * a transition. The `hermes://` VIEW intent payload is consumed as pairing.
 */
class MainActivity : ComponentActivity() {

    private val viewModel: AppViewModel by viewModels()

    private val cameraPermissionLauncher =
        registerForActivityResult(ActivityResultContracts.RequestPermission()) { }

    override fun onCreate(savedInstanceState: Bundle?) {
        super.onCreate(savedInstanceState)
        // A QR scanned by another app and shared via the hermes:// filter.
        intent?.data?.let { uri ->
            if (uri.scheme == "hermes") {
                viewModel.onQrDecoded(uri.toString())
                intent = null
            }
        }
        setContent {
            MaterialTheme {
                Surface(modifier = Modifier.fillMaxSize()) {
                    AppScreen(
                        viewModel = viewModel(),
                        onScanRequested = {
                            if (ContextCompat.checkSelfPermission(this, Manifest.permission.CAMERA)
                                == PackageManager.PERMISSION_GRANTED
                            ) {
                                true
                            } else {
                                cameraPermissionLauncher.launch(Manifest.permission.CAMERA)
                                false
                            }
                        },
                    )
                }
            }
        }
    }
}

@Composable
fun AppScreen(viewModel: AppViewModel, onScanRequested: () -> Boolean) {
    val phase by viewModel.phase.collectAsState()
    val errorText by viewModel.errorText.collectAsState()
    when (val p = phase) {
        is AppPhase.Unpaired -> PairingScreen(viewModel, errorText, onScanRequested)
        is AppPhase.NeedsPassword -> PasswordSheet(endpoint = p.endpoint, errorText = errorText, onSubmit = viewModel::submitPassword)
        is AppPhase.Connecting -> ConnectingScreen()
        is AppPhase.Ready -> ReadyScreen(viewModel, p.model)
        is AppPhase.Offline -> OfflineScreen(reason = p.reason, onRetry = { /* relaunch path re-runs via repo */ })
    }
}

/** Unpaired: scan QR (primary) + the manual fallback field. */
@Composable
fun PairingScreen(viewModel: AppViewModel, errorText: String, onScanRequested: () -> Boolean) {
    val pairingPayload by viewModel.pairingPayload.collectAsState()
    var showScanner by remember { mutableStateOf(false) }

    Column(
        modifier = Modifier
            .fillMaxSize()
            .padding(24.dp),
        horizontalAlignment = Alignment.CenterHorizontally,
        verticalArrangement = Arrangement.Center,
    ) {
        Text(text = "hermo", style = MaterialTheme.typography.headlineLarge)
        Text(
            text = "Not paired yet. Scan the gateway's QR, or paste the payload / URL below.",
            style = MaterialTheme.typography.bodyMedium,
            modifier = Modifier.padding(top = 12.dp, bottom = 16.dp),
        )
        Button(
            onClick = { if (onScanRequested()) showScanner = true },
            modifier = Modifier.fillMaxWidth(),
        ) {
            Text("Scan QR")
        }
        if (showScanner) {
            QrScanScreen(
                onDecoded = { payload ->
                    showScanner = false
                    viewModel.onQrDecoded(payload)
                },
                onCancel = { showScanner = false },
            )
        }
        OutlinedTextField(
            value = pairingPayload,
            onValueChange = viewModel::onPairingPayloadChanged,
            label = { Text("Pairing payload or gateway URL") },
            modifier = Modifier
                .fillMaxWidth()
                .padding(top = 16.dp),
        )
        Button(
            onClick = viewModel::pairFromFallback,
            modifier = Modifier
                .fillMaxWidth()
                .padding(top = 8.dp),
        ) {
            Text("Pair")
        }
        if (errorText.isNotEmpty()) {
            Text(
                text = errorText,
                color = MaterialTheme.colorScheme.error,
                modifier = Modifier.padding(top = 8.dp),
            )
        }
    }
}
