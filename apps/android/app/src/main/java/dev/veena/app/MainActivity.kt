package dev.veena.app

import android.os.Bundle
import androidx.activity.ComponentActivity
import androidx.activity.compose.setContent
import androidx.activity.enableEdgeToEdge
import androidx.activity.viewModels
import dev.veena.app.ui.VeenaScreen
import dev.veena.app.ui.VeenaTheme

class MainActivity : ComponentActivity() {
    private val viewModel: PlayerViewModel by viewModels { PlayerViewModel.Factory }

    override fun onCreate(savedInstanceState: Bundle?) {
        super.onCreate(savedInstanceState)
        enableEdgeToEdge()
        setContent {
            VeenaTheme {
                VeenaScreen(viewModel)
            }
        }
    }
}
