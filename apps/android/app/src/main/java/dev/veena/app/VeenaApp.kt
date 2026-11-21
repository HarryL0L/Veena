package dev.veena.app

import android.app.Application
import android.util.Log
import java.io.File
import uniffi.veena_ffi.Veena

/** Starts the Rust core once for the whole app. */
class VeenaApp : Application() {
    lateinit var veena: Veena
        private set

    override fun onCreate() {
        super.onCreate()
        installBundledPlugins()
        veena = Veena(filesDir.absolutePath)
    }

    /**
     * Copy the bricks shipped in the APK into the plugin folder. Bricks the
     * updater manages (listed in state.json) are left alone, so a newer
     * downloaded version is never replaced by an older bundled one.
     */
    private fun installBundledPlugins() {
        val dir = File(filesDir, "plugins").apply { mkdirs() }
        val managed = File(dir, "state.json").takeIf { it.exists() }?.readText() ?: ""
        val bundled = assets.list("plugins").orEmpty().filter { it.endsWith(".js") }
        for (name in bundled) {
            val id = name.removeSuffix(".js")
            if (managed.contains("\"$id\"")) continue
            assets.open("plugins/$name").use { input ->
                File(dir, name).outputStream().use { input.copyTo(it) }
            }
        }
        Log.i("veena", "bundled bricks: ${bundled.joinToString()}")
    }
}
