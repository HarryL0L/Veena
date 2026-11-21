package dev.veena.app

import android.annotation.SuppressLint
import android.content.Context
import android.os.Looper
import android.os.SystemClock
import android.util.Base64
import android.util.Log
import android.webkit.JavascriptInterface
import android.webkit.RenderProcessGoneDetail
import android.webkit.WebView
import android.webkit.WebViewClient
import java.net.HttpURLConnection
import java.net.URL
import java.util.concurrent.ConcurrentHashMap
import java.util.concurrent.atomic.AtomicInteger
import kotlinx.coroutines.CompletableDeferred
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.runBlocking
import kotlinx.coroutines.sync.Mutex
import kotlinx.coroutines.sync.withLock
import kotlinx.coroutines.withContext
import kotlinx.coroutines.withTimeout
import org.json.JSONArray
import org.json.JSONObject
import org.json.JSONTokener
import uniffi.veena_ffi.TokenMinter
import uniffi.veena_ffi.appLog

/**
 * Makes YouTube "proof of origin" tokens, which YouTube now asks for before
 * it serves most music. A token proves the request comes from a real
 * browser, so this runs YouTube's own check (BotGuard) in a hidden web view
 * that cannot load anything from the network itself.
 *
 * Steps: fetch the check from YouTube, run it in the web view, send its
 * answer back to get an integrity token (valid for hours), then use that to
 * make a token for each visitor id or video id a brick asks for.
 */
class PoTokenMinter(private val context: Context) : TokenMinter {
    private val lock = Mutex()
    private var session: Session? = null

    override fun mint(identifier: String): String? {
        // Bricks call this from their own threads; the web view needs the
        // main thread, so never block it.
        if (Looper.myLooper() == Looper.getMainLooper()) return null
        return runBlocking {
            lock.withLock {
                try {
                    withTimeout(30_000) {
                        val current = session?.takeIf { !it.expired && !it.gone } ?: newSession().also { session = it }
                        val result = current.call("veenaPot.mint(${JSONObject.quote(identifier)})")
                        JSONTokener(result).nextValue() as? String
                    }
                } catch (e: Exception) {
                    log("could not make a token: ${e.message}")
                    session?.close()
                    session = null
                    null
                }
            }
        }
    }

    /** Get the token maker ready ahead of the first song. Call off the main thread. */
    fun warmUp() {
        mint("warm-up")
    }

    private suspend fun newSession(): Session {
        session?.close()
        log("starting the token maker")
        val started = SystemClock.elapsedRealtime()
        val page = withContext(Dispatchers.Main) { Session.open(context) }
        try {
            val challenge = withContext(Dispatchers.IO) { parseChallenge(post("Create", JSONArray().put(REQUEST_KEY))) }
            val answer = page.call(
                "veenaPot.start(${JSONObject.quote(challenge.script)}, " +
                    "${JSONObject.quote(challenge.program)}, ${JSONObject.quote(challenge.globalName)})",
            )
            val botguard = JSONTokener(answer).nextValue() as? String ?: error("BotGuard gave no answer")
            val integrity = withContext(Dispatchers.IO) {
                JSONArray(post("GenerateIT", JSONArray().put(REQUEST_KEY).put(botguard)))
            }
            val bytes = decodeYouTubeBase64(integrity.getString(0))
            val ttlSeconds = integrity.optLong(1, 3600)
            page.call("veenaPot.setIntegrity(${JSONArray(bytes.map { it.toInt() and 0xff })})")
            // Renew ten minutes before YouTube's stated expiry.
            page.expiresAt = SystemClock.elapsedRealtime() + (ttlSeconds - 600).coerceAtLeast(60) * 1000
            log("token maker ready in ${SystemClock.elapsedRealtime() - started} ms; valid for $ttlSeconds s")
            return page
        } catch (e: Exception) {
            page.close()
            throw e
        }
    }

    /**
     * One hidden web view with BotGuard loaded. Public, because the web view
     * calls [done] and [fail] by reflection.
     */
    class Session private constructor(private val webView: WebView) {
        private val pending = ConcurrentHashMap<Int, CompletableDeferred<String>>()
        private val ids = AtomicInteger()
        var expiresAt = Long.MAX_VALUE
        val expired get() = SystemClock.elapsedRealtime() >= expiresAt
        /** The web view's page process stopped; the session can't be used. */
        @Volatile var gone = false
            private set
        private var destroyed = false

        /** Run JavaScript that may return a promise; returns the result as JSON. */
        suspend fun call(expression: String): String {
            val id = ids.incrementAndGet()
            val result = CompletableDeferred<String>()
            pending[id] = result
            withContext(Dispatchers.Main) {
                webView.evaluateJavascript(
                    "(async () => { try { const r = await ($expression); " +
                        "$BRIDGE.done($id, JSON.stringify(r === undefined ? null : r)); } " +
                        "catch (e) { $BRIDGE.fail($id, String((e && e.stack) || e)); } })()",
                    null,
                )
            }
            try {
                return result.await()
            } finally {
                pending.remove(id)
            }
        }

        @JavascriptInterface
        fun done(id: Int, json: String) {
            pending[id]?.complete(json)
        }

        @JavascriptInterface
        fun fail(id: Int, error: String) {
            pending[id]?.completeExceptionally(IllegalStateException(error.take(500)))
        }

        fun close() {
            pending.values.forEach { it.cancel() }
            webView.post { destroy() }
        }

        /** Main thread. */
        private fun destroy() {
            if (destroyed) return
            destroyed = true
            webView.destroy()
        }

        /**
         * The page process crashed or was stopped to free memory. Fail what is
         * waiting and drop this web view; the next token starts a new one.
         * Without this, Android would close the whole app.
         */
        fun renderGone(crashed: Boolean) {
            gone = true
            log("token web view stopped (${if (crashed) "crashed" else "stopped by Android"}); will restart when needed")
            val error = IllegalStateException("token web view stopped")
            pending.values.forEach { it.completeExceptionally(error) }
            destroy()
        }

        companion object {
            /** Create the web view and load an empty page "from" YouTube. Main thread. */
            @SuppressLint("SetJavaScriptEnabled")
            suspend fun open(context: Context): Session {
                val loaded = CompletableDeferred<Unit>()
                val webView = WebView(context.applicationContext)
                webView.settings.apply {
                    javaScriptEnabled = true
                    blockNetworkLoads = true
                    userAgentString = USER_AGENT
                }
                val session = Session(webView)
                webView.addJavascriptInterface(session, BRIDGE)
                webView.webViewClient = object : WebViewClient() {
                    override fun onPageFinished(view: WebView, url: String?) {
                        view.evaluateJavascript(HELPER) { loaded.complete(Unit) }
                    }

                    override fun onRenderProcessGone(view: WebView, detail: RenderProcessGoneDetail): Boolean {
                        session.renderGone(detail.didCrash())
                        loaded.completeExceptionally(IllegalStateException("token web view stopped while loading"))
                        // Handled: keep the app running.
                        return true
                    }
                }
                webView.loadDataWithBaseURL(
                    "https://www.youtube.com/",
                    "<!DOCTYPE html><html><head></head><body></body></html>",
                    "text/html",
                    "utf-8",
                    null,
                )
                loaded.await()
                return session
            }
        }
    }

    private class Challenge(val script: String, val program: String, val globalName: String)

    companion object {
        private const val TAG = "veena-pot"

        private fun log(message: String) {
            Log.i(TAG, message)
            runCatching { appLog("[tokens] $message") }
        }
        private const val BRIDGE = "VeenaPotBridge"
        private const val REQUEST_KEY = "O43z0dpjhgX20SCx4KAo"
        private const val API_KEY = "AIzaSyDyT5W0Jh49F30Pqqtyfdf7pDLFKLJoAnw"
        private const val USER_AGENT =
            "Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/131.0.0.0 Safari/537.36"

        /** Runs BotGuard and mints tokens inside the page. */
        private val HELPER = """
            window.veenaPot = {
              async start(script, program, globalName) {
                new Function(script)();
                const vm = globalThis[globalName];
                if (!vm || !vm.a) throw new Error("BotGuard not found");
                let snapshot;
                vm.a(program, (s) => { snapshot = s; }, true, undefined, () => {}, [[], []]);
                for (let i = 0; !snapshot && i < 1000; i++) await new Promise((r) => setTimeout(r, 10));
                if (!snapshot) throw new Error("BotGuard did not start");
                this.signals = [];
                return await new Promise((r) => snapshot(r, [undefined, undefined, this.signals, undefined]));
              },
              setIntegrity(bytes) {
                if (typeof this.signals?.[0] !== "function") throw new Error("no minter from BotGuard");
                this.minter = this.signals[0](new Uint8Array(bytes));
                if (typeof this.minter !== "function") throw new Error("integrity token not accepted");
              },
              mint(identifier) {
                const out = this.minter(new TextEncoder().encode(identifier));
                if (!(out instanceof Uint8Array)) throw new Error("no token made");
                let binary = "";
                for (const b of out) binary += String.fromCharCode(b);
                return btoa(binary).replace(/\+/g, "-").replace(/\//g, "_");
              },
            };
        """.trimIndent()

        /** POST to YouTube's BotGuard service; returns the response body. */
        private fun post(endpoint: String, body: JSONArray): String {
            val connection = URL("https://www.youtube.com/api/jnn/v1/$endpoint").openConnection() as HttpURLConnection
            try {
                connection.requestMethod = "POST"
                connection.connectTimeout = 15_000
                connection.readTimeout = 15_000
                connection.doOutput = true
                connection.setRequestProperty("User-Agent", USER_AGENT)
                connection.setRequestProperty("Accept", "application/json")
                connection.setRequestProperty("Content-Type", "application/json+protobuf")
                connection.setRequestProperty("x-goog-api-key", API_KEY)
                connection.setRequestProperty("x-user-agent", "grpc-web-javascript/0.1")
                connection.outputStream.use { it.write(body.toString().toByteArray()) }
                val status = connection.responseCode
                if (status != 200) error("$endpoint failed with status $status")
                return connection.inputStream.bufferedReader().use { it.readText() }
            } finally {
                connection.disconnect()
            }
        }

        /** The check's script, program and global name; sometimes scrambled. */
        private fun parseChallenge(raw: String): Challenge {
            val outer = JSONArray(raw)
            val data = if (outer.length() > 1 && outer.opt(1) is String) {
                val bytes = decodeYouTubeBase64(outer.getString(1)).map { (it + 97).toByte() }.toByteArray()
                JSONArray(bytes.decodeToString())
            } else {
                outer.getJSONArray(0)
            }
            val scripts = data.optJSONArray(1)
            val script = (0 until (scripts?.length() ?: 0)).map { scripts!!.opt(it) }.firstOrNull { it is String } as? String
                ?: error("no BotGuard script in the challenge")
            return Challenge(script, data.getString(4), data.getString(5))
        }

        /** YouTube's base64: `-` and `_` for `+` and `/`, `.` for padding. */
        private fun decodeYouTubeBase64(text: String): ByteArray =
            Base64.decode(text.replace('-', '+').replace('_', '/').replace('.', '='), Base64.DEFAULT)
    }
}
