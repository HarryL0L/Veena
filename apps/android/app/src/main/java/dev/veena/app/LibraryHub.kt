package dev.veena.app

import kotlinx.coroutines.CoroutineScope
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.SupervisorJob
import kotlinx.coroutines.delay
import kotlinx.coroutines.flow.MutableStateFlow
import kotlinx.coroutines.flow.StateFlow
import kotlinx.coroutines.flow.asStateFlow
import kotlinx.coroutines.flow.distinctUntilChanged
import kotlinx.coroutines.flow.map
import kotlinx.coroutines.launch
import kotlinx.coroutines.withContext
import uniffi.veena_ffi.PlaylistInfo
import uniffi.veena_ffi.TrackInfo
import uniffi.veena_ffi.Veena

/**
 * Liked songs, recently played and playlists, kept by the Rust core. The
 * screens follow these flows; every change goes through here so they stay
 * up to date.
 */
class LibraryHub(private val veena: Veena, player: PlayerHub) {
    private val scope = CoroutineScope(SupervisorJob() + Dispatchers.IO)

    private val _liked = MutableStateFlow<List<TrackInfo>>(emptyList())
    val liked: StateFlow<List<TrackInfo>> = _liked.asStateFlow()

    private val _history = MutableStateFlow<List<TrackInfo>>(emptyList())
    val history: StateFlow<List<TrackInfo>> = _history.asStateFlow()

    private val _cached = MutableStateFlow<List<TrackInfo>>(emptyList())
    /** Songs kept on this device for offline listening. */
    val cached: StateFlow<List<TrackInfo>> = _cached.asStateFlow()

    private val _cacheBytes = MutableStateFlow(0L)
    val cacheBytes: StateFlow<Long> = _cacheBytes.asStateFlow()

    private val _playlists = MutableStateFlow<List<PlaylistInfo>>(emptyList())
    val playlists: StateFlow<List<PlaylistInfo>> = _playlists.asStateFlow()

    init {
        scope.launch {
            _liked.value = veena.liked()
            _playlists.value = veena.playlists()
            refreshCache()
        }
        // The core adds each song to the history when it starts.
        scope.launch {
            player.state
                .map { s -> s.current?.let { s.queue.getOrNull(it.toInt()) }?.let { "${it.kind}:${it.id}" } }
                .distinctUntilChanged()
                .collect {
                    delay(500)
                    _history.value = veena.history()
                    // A song is kept once it has streamed in full, usually
                    // by the time the next one starts.
                    refreshCache()
                }
        }
    }

    fun isLiked(track: TrackInfo) = _liked.value.any { it.kind == track.kind && it.id == track.id }

    fun toggleLike(track: TrackInfo) {
        val like = !isLiked(track)
        // Show the change at once; the core saves it in the background.
        _liked.value = if (like) listOf(track) + _liked.value else _liked.value.filterNot { it.kind == track.kind && it.id == track.id }
        scope.launch {
            veena.setLiked(track, like)
            _liked.value = veena.liked()
        }
    }

    fun clearHistory() = change { clearHistory() }

    /** Take one song out of Recently played; it disappears at once. */
    fun removeFromHistory(track: TrackInfo) {
        _history.value = _history.value.filterNot { it.kind == track.kind && it.id == track.id }
        change { removeFromHistory(track) }
    }

    fun isCached(track: TrackInfo) = _cached.value.any { it.kind == track.kind && it.id == track.id }

    fun clearCache() {
        scope.launch {
            veena.clearCache()
            refreshCache()
        }
    }

    fun removeFromCache(track: TrackInfo) {
        scope.launch {
            veena.removeFromCache(track)
            refreshCache()
        }
    }

    /** Re-read the kept songs (after a song finishes or the size limit changes). */
    fun refreshCache() {
        scope.launch {
            _cached.value = veena.cachedTracks()
            _cacheBytes.value = veena.cacheUsageBytes().toLong()
        }
    }

    suspend fun playlistTracks(id: String): List<TrackInfo> = withContext(Dispatchers.IO) { veena.playlistTracks(id) }

    /** Create a playlist, optionally with a first song; returns its id. */
    suspend fun createPlaylist(name: String, first: TrackInfo? = null): String = withContext(Dispatchers.IO) {
        val id = veena.createPlaylist(name)
        first?.let { veena.addToPlaylist(id, it) }
        _playlists.value = veena.playlists()
        id
    }

    fun renamePlaylist(id: String, name: String) = change { renamePlaylist(id, name) }
    fun deletePlaylist(id: String) = change { deletePlaylist(id) }
    fun addToPlaylist(id: String, track: TrackInfo) = change { addToPlaylist(id, track) }
    fun removeFromPlaylist(id: String, index: Int) = change { removePlaylistEntry(id, index) }

    private fun Veena.removePlaylistEntry(id: String, index: Int) = removeFromPlaylist(id, index.toUInt())

    /** Run a library change in the background, then refresh what it affects. */
    private fun change(block: Veena.() -> Unit) {
        scope.launch {
            veena.block()
            _playlists.value = veena.playlists()
            _history.value = veena.history()
        }
    }
}
