package dev.veena.app

import android.content.Context
import java.util.TimeZone
import kotlinx.coroutines.flow.MutableStateFlow
import kotlinx.coroutines.flow.StateFlow
import kotlinx.coroutines.flow.asStateFlow
import uniffi.veena_ffi.Veena

enum class ThemeMode(val label: String) {
    SYSTEM("Follow system"),
    LIGHT("Light"),
    DARK("Dark"),
}

/** A part of the day, for the living background. */
enum class DayPart(val label: String) {
    DAWN("Dawn"),
    DAY("Day"),
    DUSK("Dusk"),
    NIGHT("Night"),
}

/** A season, for the living background. */
enum class Season(val label: String) {
    SPRING("Spring"),
    SUMMER("Summer"),
    AUTUMN("Autumn"),
    WINTER("Winter"),
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
    /** The animated sky and seasons on Home. */
    val ambience: Boolean = true,
    /** A fixed part of the day, or null to follow the clock. */
    val dayPart: DayPart? = null,
    /** A fixed season, or null to follow the date. */
    val season: Season? = null,
    /** Seasons follow the southern hemisphere (summer in December). */
    val southern: Boolean = false,
    /** Fireworks at New Year. */
    val specialDays: Boolean = true,
    /** Keep playing similar songs when the queue runs out. */
    val autoplay: Boolean = true,
)

class Settings(context: Context, private val veena: Veena) {
    private val prefs = context.getSharedPreferences("settings", Context.MODE_PRIVATE)

    private val _state = MutableStateFlow(
        SettingsState(
            cacheEnabled = prefs.getBoolean("cacheEnabled", true),
            cacheLimitMb = prefs.getInt("cacheLimitMb", 1024),
            offline = prefs.getBoolean("offline", false),
            theme = runCatching { ThemeMode.valueOf(prefs.getString("theme", null) ?: "") }.getOrDefault(ThemeMode.SYSTEM),
            ambience = prefs.getBoolean("ambience", true),
            dayPart = runCatching { DayPart.valueOf(prefs.getString("dayPart", null) ?: "") }.getOrNull(),
            season = runCatching { Season.valueOf(prefs.getString("season", null) ?: "") }.getOrNull(),
            southern = prefs.getBoolean("southern", guessSouthern()),
            specialDays = prefs.getBoolean("specialDays", true),
            autoplay = prefs.getBoolean("autoplay", true),
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
            .putBoolean("ambience", next.ambience)
            .putString("dayPart", next.dayPart?.name)
            .putString("season", next.season?.name)
            .putBoolean("southern", next.southern)
            .putBoolean("specialDays", next.specialDays)
            .putBoolean("autoplay", next.autoplay)
            .apply()
        apply(next)
    }

    private fun apply(s: SettingsState) {
        veena.setCacheEnabled(s.cacheEnabled)
        veena.setCacheLimitMb(s.cacheLimitMb.toUInt())
        veena.setOffline(s.offline)
        veena.setAutoplay(s.autoplay)
    }

    companion object {
        /** A first guess from the time zone; the user can change it in Settings. */
        fun guessSouthern(): Boolean {
            val zone = TimeZone.getDefault().id
            val south = listOf(
                "Australia/", "Antarctica/", "Pacific/Auckland", "Pacific/Chatham", "Pacific/Fiji",
                "America/Argentina", "America/Buenos_Aires", "America/Santiago", "America/Sao_Paulo",
                "America/Montevideo", "America/Asuncion", "America/La_Paz", "America/Lima",
                "Africa/Johannesburg", "Africa/Maputo", "Africa/Harare", "Africa/Windhoek",
                "Africa/Gaborone", "Africa/Lusaka", "Indian/Mauritius", "Indian/Reunion", "Indian/Antananarivo",
            )
            return south.any { zone.startsWith(it) }
        }

        /** Size limits offered for kept songs, in megabytes. */
        val CACHE_SIZES = listOf(512, 1024, 2048, 4096, 8192)
    }
}
