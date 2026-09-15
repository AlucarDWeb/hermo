package sh.mo

import org.junit.Assert.assertEquals
import org.junit.Assert.assertFalse
import org.junit.Assert.assertTrue
import org.junit.Test

/**
 * T13 theme policy: `resolve` decides which token set paints, from the
 * SELECTED mode + the OS setting — the phone port of Desktop's `resolveMode`
 * (themes/context.tsx:51, `system` default, Light/Dark/System). The parse of
 * the stored string is defensive: blank/unknown never crashes, it falls back
 * to System.
 */
class ThemeModeTest {

    @Test
    fun systemTracksTheOsSetting() {
        assertTrue(resolve(ThemeMode.System, systemDark = true))
        assertFalse(resolve(ThemeMode.System, systemDark = false))
    }

    @Test
    fun lightIgnoresTheOsSetting() {
        assertFalse(resolve(ThemeMode.Light, systemDark = false))
        assertFalse(resolve(ThemeMode.Light, systemDark = true))
    }

    @Test
    fun darkIgnoresTheOsSetting() {
        assertTrue(resolve(ThemeMode.Dark, systemDark = false))
        assertTrue(resolve(ThemeMode.Dark, systemDark = true))
    }

    @Test
    fun blankStoredValueParsesAsSystem() {
        assertEquals(ThemeMode.System, ThemeMode.fromStored(null))
        assertEquals(ThemeMode.System, ThemeMode.fromStored(""))
        assertEquals(ThemeMode.System, ThemeMode.fromStored("   "))
    }

    @Test
    fun unknownStoredValueParsesAsSystem() {
        assertEquals(ThemeMode.System, ThemeMode.fromStored("midnight"))
        assertEquals(ThemeMode.System, ThemeMode.fromStored("dark;rm -rf"))
    }

    @Test
    fun knownStoredValuesRoundTrip() {
        assertEquals(ThemeMode.Light, ThemeMode.fromStored("Light"))
        assertEquals(ThemeMode.Dark, ThemeMode.fromStored("dark"))
        assertEquals(ThemeMode.System, ThemeMode.fromStored("SYSTEM"))
    }
}
