package dev.veena.app

import android.content.Context
import kotlinx.coroutines.flow.MutableStateFlow
import kotlinx.coroutines.flow.StateFlow
import kotlinx.coroutines.flow.asStateFlow
import uniffi.veena_ffi.Veena

enum class ThemeMode(val label: String) {
    SYSTEM("Follow system"),
    LIGHT("Light"),
    DARK("Dark"),
}

/** The user's choices, kept on the device and passed to the core. */
data class SettingsState(
    /** Keep songs as they stream, to play them again without the internet. */
    val cacheEnabled: Boolean = true,
    /** Most space kept songs may use. */
    val cacheLimitMb: Int = 1024,
    /** Play only songs kept on this device. */
    val offline: Boolean = false,
    val theme: ThemeMode = ThemeMode.SYSTEM,
)

class Settings(context: Context, private val veena: Veena) {
    private val prefs = context.getSharedPreferences("settings", Context.MODE_PRIVATE)

    private val _state = MutableStateFlow(
        SettingsState(
            cacheEnabled = prefs.getBoolean("cacheEnabled", true),
            cacheLimitMb = prefs.getInt("cacheLimitMb", 1024),
            offline = prefs.getBoolean("offline", false),
            theme = runCatching { ThemeMode.valueOf(prefs.getString("theme", null) ?: "") }.getOrDefault(ThemeMode.SYSTEM),
        ),
    )
    val state: StateFlow<SettingsState> = _state.asStateFlow()

    init {
        apply(_state.value)
    }

    fun update(change: (SettingsState) -> SettingsState) {
        val next = change(_state.value)
        _state.value = next
        prefs.edit()
            .putBoolean("cacheEnabled", next.cacheEnabled)
            .putInt("cacheLimitMb", next.cacheLimitMb)
            .putBoolean("offline", next.offline)
            .putString("theme", next.theme.name)
            .apply()
        apply(next)
    }

    private fun apply(s: SettingsState) {
        veena.setCacheEnabled(s.cacheEnabled)
        veena.setCacheLimitMb(s.cacheLimitMb.toUInt())
        veena.setOffline(s.offline)
    }

    companion object {
        /** Size limits offered for kept songs, in megabytes. */
        val CACHE_SIZES = listOf(512, 1024, 2048, 4096, 8192)
    }
}
