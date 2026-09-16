package sh.mo

import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.padding
import androidx.compose.material3.AlertDialog
import androidx.compose.material3.Button
import androidx.compose.material3.LinearProgressIndicator
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.OutlinedButton
import androidx.compose.material3.OutlinedTextField
import androidx.compose.material3.Text
import androidx.compose.material3.TextButton
import androidx.compose.runtime.Composable
import androidx.compose.runtime.collectAsState
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.setValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.unit.dp
import sh.mo.ThemeMode
import sh.mo.ui.SessionPickerSheet

/**
 * Password sheet (PLAN §4 T6 item 6): plain field + button. The optional
 * "remember" toggle (EncryptedSharedPreferences, default off) is SKIPPED for
 * the PoC — per the brief it may be dropped if it costs more than a few
 * lines of build wiring; the dependency would add build graph weight the
 * PoC does not need. The password is typed at runtime and never persisted.
 */
@Composable
fun PasswordSheet(
    endpoint: String,
    errorText: String,
    onSubmit: (String) -> Unit,
    onLogout: () -> Unit,
) {
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
        // T14 (user: the password prompt was for the WRONG gateway and there
        // was no way back): log out re-opens pairing so the host can be
        // changed (e.g. LAN -> Tailscale).
        TextButton(
            onClick = onLogout,
            modifier = Modifier.padding(top = 4.dp),
        ) {
            Text("Wrong gateway? Log out", color = MaterialTheme.colorScheme.error)
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
 * `pickerOpen` is true (titlebar tap or `/sessions`). T13: the theme mode is
 * HANDED in (MainActivity reads it from UiPrefs) and passed down to the chat
 * — a composable renders the mode it is given, never infers one.
 */
@Composable
fun ReadyScreen(
    viewModel: AppViewModel,
    model: String,
    themeMode: ThemeMode,
    onThemeModeChange: (ThemeMode) -> Unit,
) {
    val pickerOpen by viewModel.pickerOpen.collectAsState()
    val remoteSessions by viewModel.remoteSessions.collectAsState()
    val pickerLoading by viewModel.pickerLoading.collectAsState()
    Box(modifier = Modifier.fillMaxSize()) {
        sh.mo.ui.ChatScreen(
            viewModel = viewModel,
            model = model,
            themeMode = themeMode,
            onThemeModeChange = onThemeModeChange,
        )
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

/** Offline banner with retry (the retry re-runs the resume path).
 *
 * T14 dead-end: with dead stored session ids every retry failed the same
 * way and the user was stuck ("no session could be resumed", nothing to
 * tap). The destructive "Reset sessions" wipes the LOCAL registry (the
 * pairing — host + password — and the host's chats survive) and mints a
 * fresh chat, behind a confirm dialog.
 */
@Composable
fun OfflineScreen(
    reason: String,
    onRetry: () -> Unit,
    onResetSessions: () -> Unit,
    onLogout: () -> Unit,
) {
    var resetConfirm by remember { mutableStateOf(false) }
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
        TextButton(onClick = { resetConfirm = true }, modifier = Modifier.padding(top = 4.dp)) {
            Text("Reset sessions", color = MaterialTheme.colorScheme.error)
        }
        // T14 (user: the password prompt was for the wrong gateway with no
        // way back): forget the pairing entirely and re-open pairing.
        TextButton(onClick = onLogout, modifier = Modifier.padding(top = 4.dp)) {
            Text("Log out / pair another gateway", color = MaterialTheme.colorScheme.error)
        }
    }
    if (resetConfirm) {
        AlertDialog(
            onDismissRequest = { resetConfirm = false },
            title = { Text("Reset sessions?") },
            text = {
                Text(
                    "Clears the local session list on this phone and starts a new " +
                        "chat. The chats on the host are not deleted, and the pairing " +
                        "(host + password) is kept.",
                )
            },
            confirmButton = {
                TextButton(onClick = {
                    resetConfirm = false
                    onResetSessions()
                }) { Text("Reset") }
            },
            dismissButton = {
                TextButton(onClick = { resetConfirm = false }) { Text("Cancel") }
            },
        )
    }
}
