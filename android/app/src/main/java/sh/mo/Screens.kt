package sh.mo

import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.padding
import androidx.compose.material3.Button
import androidx.compose.material3.LinearProgressIndicator
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.OutlinedButton
import androidx.compose.material3.OutlinedTextField
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
import sh.mo.ui.SessionPickerSheet

/**
 * Password sheet (PLAN §4 T6 item 6): plain field + button. The optional
 * "remember" toggle (EncryptedSharedPreferences, default off) is SKIPPED for
 * the PoC — per the brief it may be dropped if it costs more than a few
 * lines of build wiring; the dependency would add build graph weight the
 * PoC does not need. The password is typed at runtime and never persisted.
 */
@Composable
fun PasswordSheet(endpoint: String, errorText: String, onSubmit: (String) -> Unit) {
    var password by remember { mutableStateOf("") }

    Column(
        modifier = Modifier
            .fillMaxSize()
            .padding(24.dp),
        horizontalAlignment = Alignment.CenterHorizontally,
        verticalArrangement = Arrangement.Center,
    ) {
        Text(text = "Password for $endpoint", style = MaterialTheme.typography.titleMedium)
        OutlinedTextField(
            value = password,
            onValueChange = { password = it },
            label = { Text("Password") },
            modifier = Modifier
                .fillMaxWidth()
                .padding(top = 12.dp),
        )
        Button(
            onClick = { onSubmit(password) },
            enabled = password.isNotEmpty(),
            modifier = Modifier
                .fillMaxWidth()
                .padding(top = 8.dp),
        ) {
            Text("Sign in")
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

/** Connecting: one line + spinner — no fake states, no styling. */
@Composable
fun ConnectingScreen() {
    Column(
        modifier = Modifier
            .fillMaxSize()
            .padding(24.dp),
        horizontalAlignment = Alignment.CenterHorizontally,
        verticalArrangement = Arrangement.Center,
    ) {
        Text("Connecting…")
        LinearProgressIndicator(modifier = Modifier.padding(top = 16.dp))
    }
}

/**
 * Ready: the Desktop-shaped session window (T7a) — titlebar, transcript with
 * markdown, composer. Kept as the phase-screen hook MainActivity renders.
 *
 * T11: the session picker sheet overlays the chat when the view model's
 * `pickerOpen` is true (titlebar tap or `/sessions`).
 */
@Composable
fun ReadyScreen(viewModel: AppViewModel, model: String) {
    val pickerOpen by viewModel.pickerOpen.collectAsState()
    val remoteSessions by viewModel.remoteSessions.collectAsState()
    val pickerLoading by viewModel.pickerLoading.collectAsState()
    Box(modifier = Modifier.fillMaxSize()) {
        sh.mo.ui.ChatScreen(viewModel = viewModel, model = model)
        if (pickerOpen) {
            SessionPickerSheet(
                sessions = remoteSessions,
                loading = pickerLoading,
                onDismiss = viewModel::dismissSessionPicker,
                onResume = viewModel::resumeSession,
                onNewChat = viewModel::newChat,
            )
        }
    }
}

/** Offline banner with retry (the retry re-runs the resume path). */
@Composable
fun OfflineScreen(reason: String, onRetry: () -> Unit) {
    Column(
        modifier = Modifier
            .fillMaxSize()
            .padding(24.dp),
        horizontalAlignment = Alignment.CenterHorizontally,
        verticalArrangement = Arrangement.Center,
    ) {
        Text(text = "Offline: $reason")
        OutlinedButton(onClick = onRetry, modifier = Modifier.padding(top = 12.dp)) {
            Text("Retry")
        }
    }
}
