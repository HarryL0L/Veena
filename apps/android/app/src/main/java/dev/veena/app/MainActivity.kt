package dev.veena.app

import android.content.ComponentName
import android.os.Bundle
import androidx.activity.ComponentActivity
import androidx.activity.compose.setContent
import androidx.activity.enableEdgeToEdge
import androidx.activity.viewModels
import androidx.media3.session.MediaController
import androidx.media3.session.SessionToken
import com.google.common.util.concurrent.ListenableFuture
import dev.veena.app.playback.PlaybackService
import dev.veena.app.ui.VeenaScreen
import dev.veena.app.ui.VeenaTheme

class MainActivity : ComponentActivity() {
    private val viewModel: PlayerViewModel by viewModels { PlayerViewModel.Factory }
    private var controller: ListenableFuture<MediaController>? = null

    override fun onCreate(savedInstanceState: Bundle?) {
        super.onCreate(savedInstanceState)
        enableEdgeToEdge()
        setContent {
            VeenaTheme {
                VeenaScreen(viewModel)
            }
        }
    }

    /**
     * Connecting to the playback service starts it, so playback carries on
     * (with notification controls) after the app is closed.
     */
    override fun onStart() {
        super.onStart()
        val token = SessionToken(this, ComponentName(this, PlaybackService::class.java))
        controller = MediaController.Builder(this, token).buildAsync()
    }

    override fun onStop() {
        controller?.let(MediaController::releaseFuture)
        controller = null
        super.onStop()
    }
}
