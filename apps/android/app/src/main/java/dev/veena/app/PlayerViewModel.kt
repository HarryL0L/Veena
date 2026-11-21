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
import uniffi.veena_ffi.PlayerState
import uniffi.veena_ffi.TrackInfo
import uniffi.veena_ffi.RepeatMode

/** The two kinds of search results the app shows separately. */
enum class Section(val label: String) {
    MUSIC("Music"),
    VIDEOS("Videos"),
}

data class SearchUiState(
    val query: String = "",
    val results: List<TrackInfo> = emptyList(),
    val searching: Boolean = false,
    val section: Section = Section.MUSIC,
) {
    /** The results in the chosen section. */
    val shown: List<TrackInfo> get() = results.filter { sectionOf(it) == section }

    companion object {
        /** Name of the video search brick; everything else counts as music. */
        private const val VIDEO_SOURCE = "YouTube videos"

        fun sectionOf(track: TrackInfo) = if (track.source == VIDEO_SOURCE) Section.VIDEOS else Section.MUSIC
    }
}

class PlayerViewModel(private val hub: PlayerHub) : ViewModel() {
    private val _search = MutableStateFlow(SearchUiState())
    val search: StateFlow<SearchUiState> = _search.asStateFlow()

    val player: StateFlow<PlayerState> = hub.state

    /** One-off messages for a snackbar. */
    private val _messages = MutableStateFlow<String?>(null)
    val messages: StateFlow<String?> = _messages.asStateFlow()

    init {
        viewModelScope.launch {
            hub.errors.collect { _messages.value = friendly(it) }
        }
    }

    fun onQueryChange(query: String) = _search.update { it.copy(query = query) }
    fun onSectionChange(section: Section) = _search.update { it.copy(section = section) }

    fun search() {
        val query = _search.value.query.trim()
        if (query.isEmpty()) return
        _search.update { it.copy(searching = true) }
        viewModelScope.launch {
            val result = withContext(Dispatchers.IO) { runCatching { hub.veena.search(query) } }
            result.onFailure { _messages.value = it.message ?: "Search failed" }
            _search.update { it.copy(searching = false, results = result.getOrDefault(it.results)) }
        }
    }

    /** Play a result, queueing the rest of its section after it. */
    fun play(index: Int) {
        val tracks = _search.value.shown
        hub.send { play(tracks, index.toUInt()) }
    }

    fun playQueueItem(index: Int) = hub.send { playAt(index.toUInt()) }
    fun togglePause() = hub.send { togglePause() }
    fun next() = hub.send { next() }
    fun previous() = hub.send { previous() }
    fun seek(positionMs: Long) = hub.send { seek(positionMs.toULong()) }
    fun toggleShuffle() {
        val on = !player.value.shuffle
        hub.send { setShuffle(on) }
    }
    fun cycleRepeat() {
        val next = when (player.value.repeat) {
            RepeatMode.OFF -> RepeatMode.ALL
            RepeatMode.ALL -> RepeatMode.ONE
            RepeatMode.ONE -> RepeatMode.OFF
        }
        hub.send { setRepeat(next) }
    }
    fun messageShown() {
        _messages.value = null
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
            initializer { PlayerViewModel((this[APPLICATION_KEY] as VeenaApp).hub) }
        }
    }
}
