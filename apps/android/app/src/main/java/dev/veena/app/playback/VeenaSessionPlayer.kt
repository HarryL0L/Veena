package dev.veena.app.playback

import android.net.Uri
import android.os.Looper
import android.os.SystemClock
import androidx.media3.common.C
import androidx.media3.common.MediaItem
import androidx.media3.common.MediaMetadata
import androidx.media3.common.Player
import androidx.media3.common.SimpleBasePlayer
import androidx.media3.common.util.UnstableApi
import com.google.common.util.concurrent.Futures
import com.google.common.util.concurrent.ListenableFuture
import dev.veena.app.PlayerHub
import kotlin.math.abs
import kotlinx.coroutines.CoroutineScope
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.SupervisorJob
import kotlinx.coroutines.cancel
import kotlinx.coroutines.launch
import uniffi.veena_ffi.PlayState
import uniffi.veena_ffi.PlayerState
import uniffi.veena_ffi.RepeatMode

/**
 * Presents the Rust player to Android as a Media3 [Player], so the media
 * session can show it in the notification and on the lock screen. It plays
 * nothing itself: it mirrors [PlayerHub.state] and forwards commands.
 */
@UnstableApi
class VeenaSessionPlayer(private val hub: PlayerHub) : SimpleBasePlayer(Looper.getMainLooper()) {
    private val scope = CoroutineScope(SupervisorJob() + Dispatchers.Main.immediate)
    private var shown: PlayerState = hub.state.value
    private var shownAt = SystemClock.elapsedRealtime()

    init {
        scope.launch {
            hub.state.collect { next ->
                if (needsUpdate(next)) {
                    shown = next
                    shownAt = SystemClock.elapsedRealtime()
                    invalidateState()
                }
            }
        }
    }

    /**
     * Position ticks arrive several times a second. The session extrapolates
     * the position itself, so only pass on real changes and large jumps.
     */
    private fun needsUpdate(next: PlayerState): Boolean {
        if (next.copy(positionMs = 0u) != shown.copy(positionMs = 0u)) return true
        val elapsed = if (shown.state == PlayState.PLAYING) SystemClock.elapsedRealtime() - shownAt else 0
        val expected = shown.positionMs.toLong() + elapsed
        return abs(next.positionMs.toLong() - expected) > 1500
    }

    override fun getState(): State {
        val s = shown
        val playlist = s.queue.mapIndexed { index, track ->
            val duration = (if (index == s.current?.toInt()) s.durationMs else null) ?: track.durationMs
            MediaItemData.Builder("$index:${track.kind}:${track.id}")
                .setMediaItem(MediaItem.Builder().setMediaId("${track.kind}:${track.id}").build())
                .setMediaMetadata(
                    MediaMetadata.Builder()
                        .setTitle(track.title)
                        .setArtist(track.artist)
                        .setAlbumTitle(track.album)
                        .setArtworkUri(track.artwork?.let(Uri::parse))
                        .build(),
                )
                .setDurationUs(duration?.let { it.toLong() * 1000 } ?: C.TIME_UNSET)
                .build()
        }
        val playbackState = when {
            playlist.isEmpty() -> Player.STATE_IDLE
            s.state == PlayState.LOADING -> Player.STATE_BUFFERING
            s.state == PlayState.STOPPED -> Player.STATE_ENDED
            else -> Player.STATE_READY
        }
        val playing = s.state == PlayState.PLAYING || s.state == PlayState.LOADING
        return State.Builder()
            .setAvailableCommands(COMMANDS)
            .setPlayWhenReady(playing, Player.PLAY_WHEN_READY_CHANGE_REASON_USER_REQUEST)
            .setPlaybackState(playbackState)
            .setIsLoading(s.state == PlayState.LOADING)
            .setPlaylist(playlist)
            .setCurrentMediaItemIndex(if (playlist.isEmpty()) C.INDEX_UNSET else (s.current?.toInt() ?: 0))
            .setContentPositionMs(
                if (s.state == PlayState.PLAYING) {
                    PositionSupplier.getExtrapolating(s.positionMs.toLong(), 1f)
                } else {
                    PositionSupplier.getConstant(s.positionMs.toLong())
                },
            )
            .setShuffleModeEnabled(s.shuffle)
            .setRepeatMode(
                when (s.repeat) {
                    RepeatMode.OFF -> Player.REPEAT_MODE_OFF
                    RepeatMode.ALL -> Player.REPEAT_MODE_ALL
                    RepeatMode.ONE -> Player.REPEAT_MODE_ONE
                },
            )
            .build()
    }

    override fun handleSetPlayWhenReady(playWhenReady: Boolean): ListenableFuture<*> = done {
        if (playWhenReady) playCurrent() else pause()
    }

    override fun handlePrepare(): ListenableFuture<*> = Futures.immediateVoidFuture()

    override fun handleStop(): ListenableFuture<*> = done { stop() }

    override fun handleSeek(mediaItemIndex: Int, positionMs: Long, seekCommand: Int): ListenableFuture<*> = done {
        when (seekCommand) {
            Player.COMMAND_SEEK_TO_NEXT, Player.COMMAND_SEEK_TO_NEXT_MEDIA_ITEM -> next()
            Player.COMMAND_SEEK_TO_PREVIOUS, Player.COMMAND_SEEK_TO_PREVIOUS_MEDIA_ITEM -> previous()
            else -> {
                val position = if (positionMs == C.TIME_UNSET) 0L else positionMs.coerceAtLeast(0)
                if (mediaItemIndex != shown.current?.toInt()) {
                    playAt(mediaItemIndex.toUInt())
                } else {
                    seek(position.toULong())
                }
            }
        }
    }

    override fun handleSetShuffleModeEnabled(shuffleModeEnabled: Boolean): ListenableFuture<*> = done {
        setShuffle(shuffleModeEnabled)
    }

    override fun handleSetRepeatMode(repeatMode: Int): ListenableFuture<*> = done {
        setRepeat(
            when (repeatMode) {
                Player.REPEAT_MODE_ONE -> RepeatMode.ONE
                Player.REPEAT_MODE_ALL -> RepeatMode.ALL
                else -> RepeatMode.OFF
            },
        )
    }

    override fun handleRelease(): ListenableFuture<*> {
        scope.cancel()
        return Futures.immediateVoidFuture()
    }

    private fun done(command: uniffi.veena_ffi.Veena.() -> Unit): ListenableFuture<*> {
        hub.send(command)
        return Futures.immediateVoidFuture()
    }

    private companion object {
        val COMMANDS: Player.Commands = Player.Commands.Builder()
            .addAll(
                Player.COMMAND_PLAY_PAUSE,
                Player.COMMAND_PREPARE,
                Player.COMMAND_STOP,
                Player.COMMAND_SEEK_TO_DEFAULT_POSITION,
                Player.COMMAND_SEEK_IN_CURRENT_MEDIA_ITEM,
                Player.COMMAND_SEEK_TO_PREVIOUS_MEDIA_ITEM,
                Player.COMMAND_SEEK_TO_PREVIOUS,
                Player.COMMAND_SEEK_TO_NEXT_MEDIA_ITEM,
                Player.COMMAND_SEEK_TO_NEXT,
                Player.COMMAND_SEEK_TO_MEDIA_ITEM,
                Player.COMMAND_SET_SHUFFLE_MODE,
                Player.COMMAND_SET_REPEAT_MODE,
                Player.COMMAND_GET_CURRENT_MEDIA_ITEM,
                Player.COMMAND_GET_TIMELINE,
                Player.COMMAND_GET_METADATA,
                Player.COMMAND_RELEASE,
            )
            .build()
    }
}
