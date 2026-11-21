package dev.veena.app

import android.app.Application
import android.util.Log
import java.io.File
import uniffi.veena_ffi.Veena
import uniffi.veena_ffi.setTokenMinter

/** Starts the Rust core once for the whole app. */
class VeenaApp : Application() {
    lateinit var veena: Veena
        private set
    lateinit var hub: PlayerHub
        private set

    override fun onCreate() {
        super.onCreate()
        installBundledPlugins()
        // Lets YouTube bricks get proof-of-origin tokens from a hidden web view.
        setTokenMinter(PoTokenMinter(this))
        veena = Veena(filesDir.absolutePath)
        hub = PlayerHub(veena)
        BrickUpdateWorker.schedule(this)
    }

    /**
     * Copy the bricks shipped in the APK into the plugin folder, and remove
     * bricks an earlier version shipped but this one no longer does. Bricks
     * the updater manages (listed in state.json) are left alone, so a newer
     * downloaded version is never replaced or removed.
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
        dir.listFiles { file -> file.name.endsWith(".js") }.orEmpty()
            .filter { it.name !in bundled && !managed.contains("\"${it.nameWithoutExtension}\"") }
            .forEach { it.delete() }
        Log.i("veena", "bundled bricks: ${bundled.joinToString()}")
    }
}
