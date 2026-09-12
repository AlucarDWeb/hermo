package sh.mo

import org.junit.Assert.assertEquals
import org.junit.Assert.assertTrue
import org.junit.Test

/**
 * Pins the phase machine: a scripted sequence of connection/error events
 * must produce EXACTLY the expected phase sequence. If the reduction is
 * reverted/removed this fails to compile (PhaseMachine gone) or produces a
 * different sequence — no tautology.
 */
class PhaseMachineTest {

    @Test
    fun `pairing to ready then offline then relogin follows the planned sequence`() {
        val ep = "http://192.168.1.48:9123 (hermo)"
        var p: AppPhase = AppPhase.Unpaired

        // paired, no valid jar -> password asked
        p = PhaseMachine.reduce(p, PhaseMachine.ConnEvent.NeedsPassword, ep)
        assertEquals(AppPhase.NeedsPassword(ep), p)

        // user submits the password -> connecting
        p = PhaseMachine.reduce(p, PhaseMachine.ConnEvent.Connecting, ep)
        assertEquals(AppPhase.Connecting, p)

        // socket open, session opened -> Ready with the model
        p = PhaseMachine.reduce(p, PhaseMachine.ConnEvent.Open, ep)
        assertEquals(AppPhase.Connecting, p) // Open alone is not Ready
        p = PhaseMachine.ready("claude-sonnet-4-5")
        assertEquals(AppPhase.Ready("claude-sonnet-4-5"), p)

        // network drop -> Offline with the reason
        p = PhaseMachine.reduce(p, PhaseMachine.ConnEvent.Closed("timeout"), ep)
        assertEquals(AppPhase.Offline("timeout"), p)

        // retry path: connecting again, open again, ready again
        p = PhaseMachine.reduce(p, PhaseMachine.ConnEvent.Connecting, ep)
        assertEquals(AppPhase.Connecting, p)
        p = PhaseMachine.reduce(p, PhaseMachine.ConnEvent.Open, ep)
        p = PhaseMachine.ready("claude-sonnet-4-5")
        assertEquals(AppPhase.Ready("claude-sonnet-4-5"), p)
    }

    @Test
    fun `session expiry asks for the password keeping endpoint`() {
        val ep = "hermo-lan"
        var p: AppPhase = PhaseMachine.ready("m")
        p = PhaseMachine.reduce(p, PhaseMachine.ConnEvent.NeedsPassword, ep)
        assertEquals(AppPhase.NeedsPassword(ep), p)
    }

    @Test
    fun `deliberate close with no endpoint returns to unpaired`() {
        var p: AppPhase = AppPhase.NeedsPassword("x")
        p = PhaseMachine.reduce(p, PhaseMachine.ConnEvent.Closed("user logout"), "")
        assertEquals(AppPhase.Unpaired, p)
    }

    @Test
    fun `needs password with no saved endpoint cannot fake a phase`() {
        var p: AppPhase = AppPhase.Unpaired
        p = PhaseMachine.reduce(p, PhaseMachine.ConnEvent.NeedsPassword, "")
        assertEquals(AppPhase.Unpaired, p)
    }

    @Test
    fun `ready is sticky through an Open event`() {
        val p = PhaseMachine.ready("m")
        assertEquals(p, PhaseMachine.reduce(p, PhaseMachine.ConnEvent.Open, "e"))
    }

    @Test
    fun `offline carries the reason text`() {
        val p = PhaseMachine.offline("DNS failure")
        assertTrue(p is AppPhase.Offline && p.reason == "DNS failure")
    }

    /**
     * The device run showed `Model: unknown` because the header lands after
     * `open_session`. Without the Header branch the first assertion below
     * stays on the empty model and fails.
     */
    @Test
    fun `a header landing after Ready refreshes the model name`() {
        var p = PhaseMachine.ready("")
        p = PhaseMachine.reduce(p, PhaseMachine.ConnEvent.Header("claude-sonnet-4-5"), "e")
        assertEquals(AppPhase.Ready("claude-sonnet-4-5"), p)
    }

    @Test
    fun `a header never wipes the model and never leaves Ready`() {
        val ready = PhaseMachine.ready("m")
        assertEquals(ready, PhaseMachine.reduce(ready, PhaseMachine.ConnEvent.Header(""), "e"))
        val connecting: AppPhase = AppPhase.Connecting
        assertEquals(connecting, PhaseMachine.reduce(connecting, PhaseMachine.ConnEvent.Header("m"), "e"))
    }
}
