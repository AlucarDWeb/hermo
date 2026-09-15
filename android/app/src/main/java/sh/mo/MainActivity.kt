package sh.mo

import android.Manifest
import android.content.Context
import android.content.pm.PackageManager
import android.net.ConnectivityManager
import android.net.Network
import android.net.NetworkCapabilities
import android.os.Bundle
import androidx.activity.ComponentActivity
import androidx.activity.compose.setContent
import androidx.activity.result.contract.ActivityResultContracts
import androidx.activity.viewModels
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Box
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
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.setValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.unit.dp
import androidx.core.content.ContextCompat
import androidx.lifecycle.viewmodel.compose.viewModel
import sh.mo.ui.HermoTheme
import sh.mo.ThemeMode
import sh.mo.UiPrefs

/**
 * Single-activity host (T6b): renders the screen for the current [AppPhase].
 * The phase comes from the view model's flows — the composable never infers
 * a transition. The `hermes://` VIEW intent payload is consumed as pairing.
 *
 * T11 lifecycle (decision 3): `onStart` → `repo.appDidForeground()` (the
 * core's probe ping + reconnect when the socket died in background);
 * `onStop` is deliberately EMPTY — the server parks the socket 20 s and a
 * disconnect here would drop a live turn.
 *
 * T11 decision 4: while Offline, the network coming back retries the resume
 * path — one shot per network transition (ConnectivityManager callback), no
 * polling loop.
 */
class MainActivity : ComponentActivity() {

    private val viewModel: AppViewModel by viewModels()

    private val cameraPermissionLauncher =
        registerForActivityResult(ActivityResultContracts.RequestPermission()) { granted ->
            // A denied permission must be visible: the manual fallback stays
            // reachable, so this is a message, not a capability cut.
            if (!granted) viewModel.onCameraPermissionDenied()
        }

    /**
     * Decision 4's reconnect trigger: "the network is usable again" is a
     * network with a VALIDATED INTERNET capability (mere link-up without it
     * is a captive portal or a dead Wi-Fi). The guard in the view model
     * ignores the signal unless the phase is Offline.
     */
    private val networkCallback = object : ConnectivityManager.NetworkCallback() {
        override fun onCapabilitiesChanged(network: Network, caps: NetworkCapabilities) {
            if (caps.hasCapability(NetworkCapabilities.NET_CAPABILITY_VALIDATED)) {
                runOnUiThread { viewModel.onNetworkAvailable() }
            }
        }
    }

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
            // T13: the appearance mode is activity-level state — read once
            // from the adapter (UiPrefs, SharedPreferences `hermo_ui`), fed
            // to HermoTheme, and persisted on change. Process death: the
            // relaunch re-reads the stored mode, so the choice survives.
            var themeMode by remember { mutableStateOf(UiPrefs.loadThemeMode(this)) }
            HermoTheme(mode = themeMode) {
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
                        themeMode = themeMode,
                        onThemeModeChange = { mode ->
                            UiPrefs.saveThemeMode(this, mode)
                            themeMode = mode
                        },
                    )
                }
            }
        }
    }

    override fun onStart() {
        super.onStart()
        // T11 decision 3: foreground → probe ping (+ reconnect when the
        // socket died in background). ON_STOP stays empty by design.
        viewModel.onAppForeground()
        val cm = getSystemService(Context.CONNECTIVITY_SERVICE) as ConnectivityManager
        cm.registerDefaultNetworkCallback(networkCallback)
    }

    override fun onStop() {
        // No disconnect (T11 decision 3): the server parks the socket 20 s.
        // Unregister the network callback here so onStart does not stack a
        // second one every foreground.
        try {
            val cm = getSystemService(Context.CONNECTIVITY_SERVICE) as ConnectivityManager
            cm.unregisterNetworkCallback(networkCallback)
        } catch (_: IllegalArgumentException) {
            // Never registered.
        }
        super.onStop()
    }

    override fun onDestroy() {
        try {
            val cm = getSystemService(Context.CONNECTIVITY_SERVICE) as ConnectivityManager
            cm.unregisterNetworkCallback(networkCallback)
        } catch (_: IllegalArgumentException) {
            // Never registered (a failed onCreate) — teardown guard, not a
            // behaviour branch.
        }
        super.onDestroy()
    }
}

@Composable
fun AppScreen(
    viewModel: AppViewModel,
    onScanRequested: () -> Boolean,
    themeMode: ThemeMode,
    onThemeModeChange: (ThemeMode) -> Unit,
) {
    val phase by viewModel.phase.collectAsState()
    val errorText by viewModel.errorText.collectAsState()
    when (val p = phase) {
        is AppPhase.Unpaired -> PairingScreen(viewModel, errorText, onScanRequested)
        is AppPhase.NeedsPassword ->
            if (p.overlay) {
                // T11 decision 2: the ask arrived mid-session (cookie expiry,
                // 401, session kill) — the sheet opens OVER the existing
                // transcript, which stays mounted and live underneath.
                Box(modifier = Modifier.fillMaxSize()) {
                    ReadyScreen(
                        viewModel,
                        viewModel.lastReadyModel,
                        themeMode = themeMode,
                        onThemeModeChange = onThemeModeChange,
                    )
                    PasswordSheet(endpoint = p.endpoint, errorText = errorText, onSubmit = viewModel::submitPassword)
                }
            } else {
                // First pair (Unpaired → NeedsPassword): the sheet is the
                // screen — there is no transcript under it.
                PasswordSheet(endpoint = p.endpoint, errorText = errorText, onSubmit = viewModel::submitPassword)
            }
        is AppPhase.Connecting -> ConnectingScreen()
        is AppPhase.Ready ->
            ReadyScreen(
                viewModel,
                p.model,
                themeMode = themeMode,
                onThemeModeChange = onThemeModeChange,
            )
        is AppPhase.Offline ->
            OfflineScreen(
                reason = p.reason,
                onRetry = { viewModel.retryResume() },
                onResetSessions = { viewModel.resetSessionsAndRestart() },
            )
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
