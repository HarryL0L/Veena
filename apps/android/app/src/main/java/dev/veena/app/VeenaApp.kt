package dev.veena.app

import android.app.Application
import android.util.Log
import java.io.File
import kotlin.concurrent.thread
import uniffi.veena_ffi.BundledBrick
import uniffi.veena_ffi.Veena
import uniffi.veena_ffi.installBundledBricks
import uniffi.veena_ffi.setTokenMinter

/** Starts the Rust core once for the whole app. */
class VeenaApp : Application() {
    lateinit var veena: Veena
        private set
    lateinit var hub: PlayerHub
        private set
    lateinit var library: LibraryHub
        private set
    lateinit var settings: Settings
        private set

    override fun onCreate() {
        super.onCreate()
        installBundledPlugins()
        // Lets YouTube bricks get proof-of-origin tokens from a hidden web view.
        val minter = PoTokenMinter(this)
        setTokenMinter(minter)
        veena = Veena(filesDir.absolutePath, cacheDir.absolutePath)
        settings = Settings(this, veena)
        // Set up the token maker after the first screen is showing, so the
        // web view it needs doesn't slow down opening the app.
        thread(name = "token-warm-up") {
            Thread.sleep(5_000)
            minter.warmUp()
        }
        hub = PlayerHub(veena)
        library = LibraryHub(veena, hub)
        BrickUpdateWorker.schedule(this)
    }

    /**
     * Put the bricks shipped in the APK in the plugin folder. Whichever is
     * newer wins: a brick downloaded since is kept, and a newer one in the
     * APK replaces an older download (the core decides; see
     * `install_bundled_bricks`).
     */
    private fun installBundledPlugins() {
        val dir = File(filesDir, "plugins")
        val bundled = assets.list("plugins").orEmpty().filter { it.endsWith(".js") }.map { name ->
            BundledBrick(name, assets.open("plugins/$name").bufferedReader().use { it.readText() })
        }
        runCatching { installBundledBricks(dir.absolutePath, bundled) }
            .onFailure { Log.w("veena", "could not install bundled bricks: ${it.message}") }
        Log.i("veena", "bundled bricks: ${bundled.joinToString { it.name }}")
    }
}
