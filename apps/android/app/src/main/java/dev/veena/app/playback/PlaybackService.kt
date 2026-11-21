package dev.veena.app.playback

import android.app.PendingIntent
import android.content.BroadcastReceiver
import android.content.Context
import android.content.Intent
import android.content.IntentFilter
import android.media.AudioAttributes
import android.media.AudioFocusRequest
import android.media.AudioManager
import androidx.core.content.ContextCompat
import androidx.media3.common.util.UnstableApi
import androidx.media3.session.MediaSession
import androidx.media3.session.MediaSessionService
import dev.veena.app.MainActivity
import dev.veena.app.PlayerHub
import dev.veena.app.VeenaApp
import kotlinx.coroutines.CoroutineScope
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.SupervisorJob
import kotlinx.coroutines.cancel
import kotlinx.coroutines.flow.distinctUntilChanged
import kotlinx.coroutines.flow.map
import kotlinx.coroutines.launch
import uniffi.veena_ffi.PlayState

/**
 * Keeps Veena playing when the app is in the background, and puts the
 * controls in the notification, on the lock screen and on headphones.
 */
@UnstableApi
class PlaybackService : MediaSessionService() {
    private var session: MediaSession? = null
    private lateinit var hub: PlayerHub
    private lateinit var audio: AudioManager
    private val scope = CoroutineScope(SupervisorJob() + Dispatchers.Main.immediate)

    private var hasFocus = false
    private var pausedByFocusLoss = false
    private var ducked = false
    private var volumeBeforeDuck = 1f
    private var noisyRegistered = false

    override fun onCreate() {
        super.onCreate()
        hub = (application as VeenaApp).hub
        audio = getSystemService(AudioManager::class.java)
        val openApp = PendingIntent.getActivity(
            this,
            0,
            Intent(this, MainActivity::class.java),
            PendingIntent.FLAG_IMMUTABLE or PendingIntent.FLAG_UPDATE_CURRENT,
        )
        session = MediaSession.Builder(this, VeenaSessionPlayer(hub))
            .setSessionActivity(openApp)
            .build()

        scope.launch {
            hub.state.map { it.state }.distinctUntilChanged().collect(::onPlayState)
        }
    }

    override fun onGetSession(controllerInfo: MediaSession.ControllerInfo): MediaSession? = session

    override fun onDestroy() {
        scope.cancel()
        setNoisyReceiver(false)
        abandonFocus()
        session?.run {
            player.release()
            release()
        }
        session = null
        super.onDestroy()
    }

    // ---------------------------------------------------------- Audio focus

    private val focusRequest by lazy {
        AudioFocusRequest.Builder(AudioManager.AUDIOFOCUS_GAIN)
            .setAudioAttributes(
                AudioAttributes.Builder()
                    .setUsage(AudioAttributes.USAGE_MEDIA)
                    .setContentType(AudioAttributes.CONTENT_TYPE_MUSIC)
                    .build(),
            )
            .setWillPauseWhenDucked(false)
            .setOnAudioFocusChangeListener(::onFocusChange)
            .build()
    }

    private fun onPlayState(state: PlayState) {
        when (state) {
            PlayState.PLAYING, PlayState.LOADING -> {
                if (!hasFocus) {
                    hasFocus = audio.requestAudioFocus(focusRequest) == AudioManager.AUDIOFOCUS_REQUEST_GRANTED
                }
                setNoisyReceiver(true)
            }
            PlayState.PAUSED -> setNoisyReceiver(false)
            PlayState.STOPPED -> {
                setNoisyReceiver(false)
                abandonFocus()
            }
        }
    }

    /** Another app wants to play sound (a call, a video, directions). */
    private fun onFocusChange(change: Int) {
        val playing = hub.state.value.state.let { it == PlayState.PLAYING || it == PlayState.LOADING }
        when (change) {
            AudioManager.AUDIOFOCUS_LOSS -> {
                hasFocus = false
                pausedByFocusLoss = false
                hub.send { pause() }
            }
            AudioManager.AUDIOFOCUS_LOSS_TRANSIENT -> {
                if (playing) {
                    pausedByFocusLoss = true
                    hub.send { pause() }
                }
            }
            AudioManager.AUDIOFOCUS_LOSS_TRANSIENT_CAN_DUCK -> {
                if (playing && !ducked) {
                    ducked = true
                    volumeBeforeDuck = hub.state.value.volume
                    hub.send { setVolume(volumeBeforeDuck * 0.3f) }
                }
            }
            AudioManager.AUDIOFOCUS_GAIN -> {
                hasFocus = true
                if (ducked) {
                    ducked = false
                    hub.send { setVolume(volumeBeforeDuck) }
                }
                if (pausedByFocusLoss) {
                    pausedByFocusLoss = false
                    hub.send { playCurrent() }
                }
            }
        }
    }

    private fun abandonFocus() {
        if (hasFocus) audio.abandonAudioFocusRequest(focusRequest)
        hasFocus = false
    }

    // ------------------------------------------- Headphones unplugged

    private val noisyReceiver = object : BroadcastReceiver() {
        override fun onReceive(context: Context, intent: Intent) {
            if (intent.action == AudioManager.ACTION_AUDIO_BECOMING_NOISY) hub.send { pause() }
        }
    }

    private fun setNoisyReceiver(on: Boolean) {
        if (on == noisyRegistered) return
        if (on) {
            ContextCompat.registerReceiver(
                this,
                noisyReceiver,
                IntentFilter(AudioManager.ACTION_AUDIO_BECOMING_NOISY),
                ContextCompat.RECEIVER_NOT_EXPORTED,
            )
        } else {
            unregisterReceiver(noisyReceiver)
        }
        noisyRegistered = on
    }
}
