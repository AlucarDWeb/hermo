package sh.mo

import android.app.Application
import uniffi.hermes_core.HermesCore

/**
 * Application entry point. Creates the single [HermesCore] instance bound to
 * the app's private files directory (cookie jar + session registry live
 * there) and exposes it to the rest of the app. Stream B wires this into the
 * GatewayRepository; until then the core is created eagerly and owned here.
 */
class HermesApp : Application() {

    lateinit var core: HermesCore
        private set

    override fun onCreate() {
        super.onCreate()
        app = this
        // Created exactly once per process (PLAN §4 T6 item 4).
        core = HermesCore(filesDir.absolutePath)
    }

    companion object {
        private var app: HermesApp? = null

        /** The process-wide HermesCore, created in [onCreate]. */
        fun core(): HermesCore = requireNotNull(app).core
    }
}
