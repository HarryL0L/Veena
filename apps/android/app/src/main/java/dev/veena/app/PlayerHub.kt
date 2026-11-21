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
import kotlinx.coroutines.launch
import uniffi.veena_ffi.PlayerListener
import uniffi.veena_ffi.PlayerState
import uniffi.veena_ffi.Veena

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
                _errors.tryEmit(message)
            }
        })
    }

    /** Run a player command off the main thread. */
    fun send(command: Veena.() -> Unit) {
        scope.launch { veena.command() }
    }
}
