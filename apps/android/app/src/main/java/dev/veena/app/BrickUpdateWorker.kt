package dev.veena.app

import android.content.Context
import android.util.Log
import androidx.work.Constraints
import androidx.work.CoroutineWorker
import androidx.work.ExistingPeriodicWorkPolicy
import androidx.work.NetworkType
import androidx.work.PeriodicWorkRequestBuilder
import androidx.work.WorkManager
import androidx.work.WorkerParameters
import java.util.concurrent.TimeUnit
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.withContext

/**
 * Checks once a day for new signed bricks and installs them. The app itself
 * is never updated by this, only the bricks.
 */
class BrickUpdateWorker(context: Context, params: WorkerParameters) : CoroutineWorker(context, params) {
    override suspend fun doWork(): Result = withContext(Dispatchers.IO) {
        val veena = (applicationContext as VeenaApp).veena
        runCatching { veena.updatePlugins(BuildConfig.UPDATE_URL, BuildConfig.UPDATE_KEY) }
            .fold(
                onSuccess = {
                    Log.i("veena", "brick update: $it")
                    Result.success()
                },
                onFailure = {
                    Log.w("veena", "brick update failed: ${it.message}")
                    Result.retry()
                },
            )
    }

    companion object {
        /** Schedule the daily check, if this build has an update address. */
        fun schedule(context: Context) {
            if (BuildConfig.UPDATE_URL.isEmpty() || BuildConfig.UPDATE_KEY.isEmpty()) return
            val request = PeriodicWorkRequestBuilder<BrickUpdateWorker>(1, TimeUnit.DAYS)
                .setConstraints(Constraints.Builder().setRequiredNetworkType(NetworkType.CONNECTED).build())
                .build()
            WorkManager.getInstance(context)
                .enqueueUniquePeriodicWork("brick-updates", ExistingPeriodicWorkPolicy.KEEP, request)
        }
    }
}
