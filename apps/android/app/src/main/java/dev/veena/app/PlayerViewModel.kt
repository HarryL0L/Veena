package dev.veena.app

import androidx.lifecycle.ViewModel
import androidx.lifecycle.ViewModelProvider
import androidx.lifecycle.viewModelScope
import androidx.lifecycle.viewmodel.initializer
import androidx.lifecycle.viewmodel.viewModelFactory
import androidx.lifecycle.ViewModelProvider.AndroidViewModelFactory.Companion.APPLICATION_KEY
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.flow.MutableStateFlow
import kotlinx.coroutines.flow.StateFlow
import kotlinx.coroutines.flow.asStateFlow
import kotlinx.coroutines.flow.update
import kotlinx.coroutines.launch
import kotlinx.coroutines.withContext
import uniffi.veena_ffi.PlayerListener
import uniffi.veena_ffi.PlayerState
import uniffi.veena_ffi.TrackInfo
import uniffi.veena_ffi.Veena

data class SearchUiState(
    val query: String = "",
    val results: List<TrackInfo> = emptyList(),
    val searching: Boolean = false,
)

class PlayerViewModel(private val veena: Veena) : ViewModel() {
    private val _search = MutableStateFlow(SearchUiState())
    val search: StateFlow<SearchUiState> = _search.asStateFlow()

    private val _player = MutableStateFlow(veena.state())
    val player: StateFlow<PlayerState> = _player.asStateFlow()

    /** One-off messages for a snackbar. */
    private val _messages = MutableStateFlow<String?>(null)
    val messages: StateFlow<String?> = _messages.asStateFlow()

    init {
        veena.setListener(object : PlayerListener {
            override fun onState(state: PlayerState) {
                _player.value = state
            }

            override fun onError(message: String) {
                _messages.value = friendly(message)
            }
        })
    }

    fun onQueryChange(query: String) = _search.update { it.copy(query = query) }

    fun search() {
        val query = _search.value.query.trim()
        if (query.isEmpty()) return
        _search.update { it.copy(searching = true) }
        viewModelScope.launch {
            val result = withContext(Dispatchers.IO) { runCatching { veena.search(query) } }
            result.onFailure { _messages.value = it.message ?: "Search failed" }
            _search.update { it.copy(searching = false, results = result.getOrDefault(it.results)) }
        }
    }

    fun play(index: Int) {
        val tracks = _search.value.results
        viewModelScope.launch(Dispatchers.IO) { veena.play(tracks, index.toUInt()) }
    }

    fun playQueueItem(index: Int) = io { veena.playAt(index.toUInt()) }
    fun togglePause() = io { veena.togglePause() }
    fun next() = io { veena.next() }
    fun previous() = io { veena.previous() }
    fun seek(positionMs: Long) = io { veena.seek(positionMs.toULong()) }
    fun messageShown() {
        _messages.value = null
    }

    private fun io(block: () -> Unit) {
        viewModelScope.launch(Dispatchers.IO) { block() }
    }

    companion object {
        /** A short, plain message for an error from the core. */
        fun friendly(message: String): String {
            val title = Regex("^cannot play (.+?): ").find(message)?.groupValues?.get(1)
            val what = if (title != null) "Can't play \"$title\"" else "Something went wrong"
            val why = when {
                "proof-of-origin" in message || "403 at the end" in message ->
                    "YouTube only allows this music track in its own apps for now."
                "403 at the start" in message || "not available" in message ->
                    "YouTube won't serve this one here."
                "nothing installed can play" in message -> "No installed brick can play it."
                "Unable to resolve host" in message || "network" in message.lowercase() ->
                    "Check your internet connection."
                else -> null
            }
            return listOfNotNull(what, why).joinToString(". ")
        }

        val Factory: ViewModelProvider.Factory = viewModelFactory {
            initializer { PlayerViewModel((this[APPLICATION_KEY] as VeenaApp).veena) }
        }
    }
}
