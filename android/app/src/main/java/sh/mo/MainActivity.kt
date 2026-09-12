package sh.mo

import android.os.Bundle
import androidx.activity.ComponentActivity
import androidx.activity.compose.setContent
import androidx.activity.viewModels
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.padding
import androidx.compose.material3.Button
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.OutlinedTextField
import androidx.compose.material3.Surface
import androidx.compose.material3.Text
import androidx.compose.runtime.Composable
import androidx.compose.runtime.collectAsState
import androidx.compose.runtime.getValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.res.stringResource
import androidx.compose.ui.unit.dp
import androidx.lifecycle.viewmodel.compose.viewModel

/**
 * Single-activity host. Renders the screen for the current [AppPhase] from
 * the [AppViewModel]; today that is the real Unpaired phase (stream B adds
 * the later phases).
 */
class MainActivity : ComponentActivity() {

    private val viewModel: AppViewModel by viewModels()

    override fun onCreate(savedInstanceState: Bundle?) {
        super.onCreate(savedInstanceState)
        setContent {
            MaterialTheme {
                Surface(modifier = Modifier.fillMaxSize()) {
                    AppScreen(viewModel = viewModel())
                }
            }
        }
    }
}

@Composable
fun AppScreen(viewModel: AppViewModel) {
    val phase by viewModel.phase.collectAsState()
    val pairingPayload by viewModel.pairingPayload.collectAsState()
    when (phase) {
        is AppPhase.Unpaired -> UnpairedScreen(
            pairingPayload = pairingPayload,
            onPairingPayloadChanged = viewModel::onPairingPayloadChanged,
            onPair = viewModel::onPairRequested,
            pairEnabled = viewModel.pairingEnabled,
        )
    }
}

/**
 * The Unpaired phase: product name plus the pairing entry.
 *
 * Pairing has two paths: scanning the QR the host displays (the primary one —
 * the scan screen lands in stream B with a no-GMS CameraX + ZXing decoder) and
 * the manual fallback below (paste a `hermes://connect?...` payload or type the
 * gateway URL + username, which the core's `parse_qr_payload` accepts). State
 * comes from the view model; `pairEnabled` is false until stream B wires the
 * action, so the button is visibly disabled instead of silently dead.
 */
@Composable
fun UnpairedScreen(
    pairingPayload: String,
    onPairingPayloadChanged: (String) -> Unit,
    onPair: () -> Unit,
    pairEnabled: Boolean,
) {
    Column(
        modifier = Modifier
            .fillMaxSize()
            .padding(24.dp),
        horizontalAlignment = Alignment.CenterHorizontally,
        verticalArrangement = Arrangement.Center,
    ) {
        Text(
            text = stringResource(R.string.unpaired_title),
            style = MaterialTheme.typography.headlineLarge,
        )
        Text(
            text = stringResource(R.string.unpaired_message),
            style = MaterialTheme.typography.bodyMedium,
            modifier = Modifier.padding(top = 12.dp, bottom = 24.dp),
        )
        OutlinedTextField(
            value = pairingPayload,
            onValueChange = onPairingPayloadChanged,
            label = { Text(stringResource(R.string.unpaired_payload_label)) },
            modifier = Modifier.fillMaxWidth(),
            singleLine = false,
        )
        Button(
            onClick = onPair,
            enabled = pairEnabled,
            modifier = Modifier
                .fillMaxWidth()
                .padding(top = 16.dp),
        ) {
            Text(text = stringResource(R.string.unpaired_cta))
        }
        if (!pairEnabled) {
            Text(
                text = stringResource(R.string.pairing_not_wired),
                style = MaterialTheme.typography.bodySmall,
                modifier = Modifier.padding(top = 8.dp),
            )
        }
    }
}
