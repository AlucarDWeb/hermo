package sh.mo

/**
 * T13 theme policy (pure — no Android imports): the same three modes as
 * Desktop's `THEME_MODES` (apps/desktop/src/app/command-palette/index.tsx:451)
 * with `resolveMode` (apps/desktop/src/themes/context.tsx:51) as the contract.
 * `system` is the default and tracks the OS setting the way Desktop tracks
 * `prefers-color-scheme`. Pure policy: no Android imports — the adapter
 * persists the string (UiPrefs, SharedPreferences) and the composables only
 * render the resolved outcome.
 */
enum class ThemeMode {
    Light,
    Dark,
    System;

    companion object {
        /** Stored-preference parse: blank or unknown value → System. */
        fun fromStored(raw: String?): ThemeMode =
            values().firstOrNull { it.name.equals(raw?.trim(), ignoreCase = true) } ?: System
    }
}

/** true = the dark token set paints (Desktop's `resolveMode`). */
fun resolve(mode: ThemeMode, systemDark: Boolean): Boolean = when (mode) {
    ThemeMode.Light -> false
    ThemeMode.Dark -> true
    ThemeMode.System -> systemDark
}
