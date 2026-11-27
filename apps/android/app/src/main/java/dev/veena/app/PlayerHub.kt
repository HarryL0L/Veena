package dev.veena.app

import kotlinx.coroutines.CoroutineScope
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.SupervisorJob
import kotlinx.coroutines.flow.MutableSharedFlow
import kotlinx.coroutines.flow.MutableStateFlow
import kotlinx.coroutines.flow.SharedFlow
import kotlinx.coroutines.flow.StateFlow
import kotlinx.coroutines.flow.asSharedFlow
import kotlinx.coroutines.flow.asStateFlow
import kotlinx.coroutines.Job
import kotlinx.coroutines.delay
import kotlinx.coroutines.flow.first
import kotlinx.coroutines.launch
import uniffi.veena_ffi.PlayerListener
import uniffi.veena_ffi.PlayerState
import uniffi.veena_ffi.Veena
import uniffi.veena_ffi.appLog

/**
 * The one place that listens to the Rust player. The screen and the media
 * session (notification, lock screen, headphones) both follow [state], so
 * they always agree.
 */
class PlayerHub(val veena: Veena) {
    private val _state = MutableStateFlow(veena.state())
    val state: StateFlow<PlayerState> = _state.asStateFlow()

    private val _errors = MutableSharedFlow<String>(extraBufferCapacity = 8)
    val errors: SharedFlow<String> = _errors.asSharedFlow()

    private val scope = CoroutineScope(SupervisorJob() + Dispatchers.IO)

    init {
        veena.setListener(object : PlayerListener {
            override fun onState(state: PlayerState) {
                _state.value = state
            }

            override fun onError(message: String) {
                runCatching { appLog("[player] $message") }
                _errors.tryEmit(message)
            }
        })
    }

    // ------------------------------------------------------------ Sleep timer

    private val _sleep = MutableStateFlow<SleepTimer?>(null)
    /** The sleep timer, if one is set. */
    val sleep: StateFlow<SleepTimer?> = _sleep.asStateFlow()
    private var sleepJob: Job? = null

    /** Pause after [minutes], or at the end of the current song when null. Cancels any earlier timer. */
    fun setSleepTimer(minutes: Int?) {
        sleepJob?.cancel()
        if (minutes != null) {
            send { setPauseAtEnd(false) }
            val endsAt = System.currentTimeMillis() + minutes * 60_000L
            _sleep.value = SleepTimer(endsAt = endsAt)
            sleepJob = scope.launch {
                delay(minutes * 60_000L)
                veena.pause()
                _sleep.value = null
            }
        } else {
            // The player itself pauses when the song ends (also with
            // repeat-one); the next song waits, loaded and paused.
            _sleep.value = SleepTimer(endsAt = null)
            sleepJob = scope.launch {
                veena.setPauseAtEnd(true)
                state.first { it.pauseAtEnd }
                state.first { !it.pauseAtEnd }
                _sleep.value = null
            }
        }
    }

    fun cancelSleepTimer() {
        sleepJob?.cancel()
        send { setPauseAtEnd(false) }
        _sleep.value = null
    }

    /** Run a player command off the main thread. */
    fun send(command: Veena.() -> Unit) {
        scope.launch { veena.command() }
    }
}

/** When playback will pause: at [endsAt] (milliseconds since 1970), or at the end of the song when null. */
data class SleepTimer(val endsAt: Long?)
