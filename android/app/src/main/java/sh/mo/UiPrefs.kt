package sh.mo

import android.content.Context

/**
 * Adapter (persistence layer): the appearance mode lives in ORDINARY
 * `SharedPreferences` — it is a UI preference, not a secret, so no
 * EncryptedSharedPreferences. File `hermo_ui`, key `theme_mode`; the string
 * parse and its defensive fallbacks are the pure policy in [ThemeMode].
 */
object UiPrefs {
    private const val FILE = "hermo_ui"
    private const val KEY_THEME_MODE = "theme_mode"

    fun loadThemeMode(context: Context): ThemeMode =
        ThemeMode.fromStored(
            context.getSharedPreferences(FILE, Context.MODE_PRIVATE)
                .getString(KEY_THEME_MODE, null),
        )

    fun saveThemeMode(context: Context, mode: ThemeMode) {
        context.getSharedPreferences(FILE, Context.MODE_PRIVATE)
            .edit()
            .putString(KEY_THEME_MODE, mode.name)
            .apply()
    }
}
