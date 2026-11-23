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
import kotlinx.coroutines.flow.collectLatest
import kotlinx.coroutines.flow.distinctUntilChanged
import kotlinx.coroutines.flow.map
import kotlinx.coroutines.flow.update
import kotlinx.coroutines.launch
import kotlinx.coroutines.withContext
import uniffi.veena_ffi.LyricsInfo
import uniffi.veena_ffi.PlayerState
import uniffi.veena_ffi.TrackInfo
import uniffi.veena_ffi.RepeatMode
import uniffi.veena_ffi.appLog

/** A short message for the snackbar; errors offer a way to the logs. */
data class UiMessage(val text: String, val error: Boolean = false)

/** Lyrics for the current song, as the player screen shows them. */
sealed interface LyricsUi {
    data object None : LyricsUi
    data object Loading : LyricsUi
    data object NotFound : LyricsUi
    data class Found(val lyrics: LyricsInfo) : LyricsUi
}

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

class PlayerViewModel(private val hub: PlayerHub, val library: LibraryHub, private val settingsStore: Settings) : ViewModel() {
    val settings: StateFlow<SettingsState> = settingsStore.state

    fun updateSettings(change: (SettingsState) -> SettingsState) {
        settingsStore.update(change)
        library.refreshCache()
    }

    /** Installed bricks, for the settings page. */
    fun bricks() = hub.veena.plugins()

    /** Look for brick updates now; [done] gets the result on the main thread. */
    fun checkBricks(done: (String) -> Unit) {
        viewModelScope.launch {
            done(withContext(Dispatchers.IO) { BrickUpdateWorker.checkNow(hub.veena) })
        }
    }

    private val _search = MutableStateFlow(SearchUiState())
    val search: StateFlow<SearchUiState> = _search.asStateFlow()

    val player: StateFlow<PlayerState> = hub.state

    /** One-off messages for a snackbar. */
    private val _messages = MutableStateFlow<UiMessage?>(null)
    val messages: StateFlow<UiMessage?> = _messages.asStateFlow()

    private val _lyrics = MutableStateFlow<LyricsUi>(LyricsUi.None)
    /** Lyrics for the song playing now, looked up when it changes. */
    val lyrics: StateFlow<LyricsUi> = _lyrics.asStateFlow()

    init {
        viewModelScope.launch {
            hub.state
                .map { s -> s.current?.let { s.queue.getOrNull(it.toInt()) } }
                .distinctUntilChanged { a, b -> a?.kind == b?.kind && a?.id == b?.id }
                .collectLatest { track ->
                    if (track == null) {
                        _lyrics.value = LyricsUi.None
                        return@collectLatest
                    }
                    _lyrics.value = LyricsUi.Loading
                    val found = withContext(Dispatchers.IO) { runCatching { hub.veena.lyrics(track) }.getOrNull() }
                    _lyrics.value = found?.let { LyricsUi.Found(it) } ?: LyricsUi.NotFound
                }
        }
        viewModelScope.launch {
            hub.errors.collect { _messages.value = UiMessage(friendly(it), error = true) }
        }
    }

    fun onQueryChange(query: String) = _search.update { it.copy(query = query) }
    fun onSectionChange(section: Section) = _search.update { it.copy(section = section) }

    fun search() {
        val query = _search.value.query.trim()
        if (query.isEmpty()) return
        if (settings.value.offline) {
            // Offline: look through the songs kept on this device.
            val words = query.lowercase().split(Regex("\\s+"))
            val found = library.cached.value.filter { t ->
                val text = listOfNotNull(t.title, t.artist, t.album).joinToString(" ").lowercase()
                words.all { it in text }
            }
            _search.update { it.copy(results = found, searching = false, section = Section.MUSIC) }
            if (found.isEmpty()) _messages.value = UiMessage("No saved songs match \"$query\"")
            return
        }
        _search.update { it.copy(searching = true) }
        viewModelScope.launch {
            val result = withContext(Dispatchers.IO) { runCatching { hub.veena.search(query) } }
            result.onFailure {
                runCatching { appLog("[search] ${it.message}") }
                _messages.value = UiMessage(it.message ?: "Search failed", error = true)
            }
            _search.update { it.copy(searching = false, results = result.getOrDefault(it.results)) }
        }
    }

    /**
     * Play a result. With autoplay on, similar songs follow it like a radio;
     * otherwise the rest of its section is queued after it.
     */
    fun play(index: Int) {
        val tracks = _search.value.shown
        hub.send { playSearchResult(tracks, index.toUInt()) }
    }

    /** Play a list (an album, playlist, liked songs...) from one of its songs. */
    fun playList(tracks: List<TrackInfo>, index: Int = 0) {
        if (tracks.isEmpty()) return
        hub.send {
            setShuffle(false)
            play(tracks, index.coerceIn(0, tracks.lastIndex).toUInt())
        }
    }

    /** Play a list in random order. */
    fun shuffleList(tracks: List<TrackInfo>) {
        if (tracks.isEmpty()) return
        hub.send {
            setShuffle(true)
            play(tracks, tracks.indices.random().toUInt())
        }
    }

    fun toggleLike(track: TrackInfo) = library.toggleLike(track)

    /** Add a song to a playlist, or to a new one when [playlistId] is null. */
    fun addToPlaylist(track: TrackInfo, playlistId: String?, newName: String = "") {
        if (playlistId != null) {
            library.addToPlaylist(playlistId, track)
            val name = library.playlists.value.firstOrNull { it.id == playlistId }?.name ?: "the playlist"
            _messages.value = UiMessage("Added to $name")
        } else {
            viewModelScope.launch {
                library.createPlaylist(newName, track)
                _messages.value = UiMessage("Created \"${newName.ifBlank { "New playlist" }}\"")
            }
        }
    }

    fun playQueueItem(index: Int) = hub.send { playAt(index.toUInt()) }
    fun playNext(track: TrackInfo) {
        hub.send { playNext(track) }
        _messages.value = UiMessage("Playing \"${track.title}\" next")
    }
    fun addToQueue(track: TrackInfo) {
        hub.send { enqueue(listOf(track)) }
        _messages.value = UiMessage("Added \"${track.title}\" to the queue")
    }
    fun removeFromQueue(index: Int) = hub.send { remove(index.toUInt()) }
    fun moveInQueue(from: Int, to: Int) = hub.send { moveTrack(from.toUInt(), to.toUInt()) }

    val sleep = hub.sleep
    fun setSleepTimer(minutes: Int?) = hub.setSleepTimer(minutes)
    fun cancelSleepTimer() = hub.cancelSleepTimer()
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
            initializer { (this[APPLICATION_KEY] as VeenaApp).let { PlayerViewModel(it.hub, it.library, it.settings) } }
        }
    }
}
