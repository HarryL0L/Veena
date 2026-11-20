# Writing a Veena plugin

A plugin is a brick: one JavaScript file that plugs into Veena's slots.
Veena runs it in a sandbox and can update it without an app update.

---

## Slots

| Slot | Fill it by exporting | Job |
|---|---|---|
| **Search** | `search` (and optionally `playlist`) | Find music and return tracks |
| **Audio links** | `resolve` | Turn a track into a playable link |

A plugin fills one slot or both. Tracks carry a **kind**, such as `youtube`, that describes the content rather than the plugin that found it.
Any audio-link brick that handles that kind can play the track. When several can, Veena tries them in order and falls back to the next if one fails.

---

## Quick start

```js
export const meta = {
  id: "example",              // lowercase letters, digits, hyphens
  name: "Example Music",
  version: "1.0.0",
  api: 1,                     // host API version
  kinds: ["example"],         // content kinds; defaults to the id
  domains: ["example.com"],   // the only hosts this plugin may contact
};

export async function search(query) {
  const res = await veena.http.fetch(`https://example.com/find?q=${encodeURIComponent(query)}`);
  return res.json().items.map((item) => ({
    id: item.id,
    title: item.name,
    artist: item.artist,
    durationMs: item.seconds * 1000,
  }));
}

export async function resolve(track) {
  return [{ url: `https://cdn.example.com/${track.id}.m4a`, mime: "audio/mp4" }];
}
```

Save it as `example.js` in the plugin folder, then:

```sh
veena plugins                 # check that it loaded
veena find "artist name"      # search with it
```

The plugin folder is `~/.local/share/veena/plugins`. Use `--plugins <folder>` to choose another.

---

## Exports

| Export | Required | Receives | Returns |
|---|---|---|---|
| `meta` | Yes | | Plugin details (see above) |
| `search(query)` | For the search slot | Search text | List of tracks |
| `playlist(id)` | No | Playlist or album id | List of tracks |
| `resolve(track)` | For the audio-link slot | A track of one of `meta.kinds` | One stream, or a list of streams (best first) |

At least one of `search` and `resolve` is required. Functions may be `async` or plain.

**Track**

| Field | Type | Notes |
|---|---|---|
| `id` | string | Required. Id within its kind, e.g. a video id. |
| `title` | string | Required. |
| `kind` | string | Optional. Defaults to the first of `meta.kinds`. |
| `artist`, `album`, `artwork` | string | Optional. |
| `durationMs` | number | Optional. |

**Stream**

| Field | Type | Notes |
|---|---|---|
| `url` | string | Required. Must be on an allowed domain. |
| `mime` | string | Optional, e.g. `audio/mp4`. Helps decoding. |
| `bitrate` | number | Optional, bits per second. |
| `headers` | object | Optional request headers for playback. |

---

## Host API

| API | Description |
|---|---|
| `veena.http.fetch(url, { method, headers, body })` | Returns `{ status, ok, url, headers, text, json() }`. `GET`, `HEAD` and `POST`. |
| `veena.cache.get(key)` | A cached string, or `undefined`. |
| `veena.cache.set(key, value, ttlSeconds?)` | Store a string, optionally expiring. |
| `veena.cache.delete(key)` | Remove a cached value. |
| `veena.config` | Read-only settings delivered with the plugin. |
| `veena.attempt(fn, ...args)` | Call code you don't control. Returns `{ ok, value }` or `{ ok: false, error }`, including for loop and recursion limits, which `try`/`catch` cannot catch. |
| `veena.log(...)`, `console.log(...)` | Write to Veena's log. |
| `veena.apiVersion` | Host API version (`1`). |

Standard JavaScript is available: `JSON`, `RegExp`, `Promise`, `eval` and so on.
Browser APIs such as `URL`, `URLSearchParams` and `setTimeout` are not.

---

## Sandbox

| Rule | Detail |
|---|---|
| Network | Only `meta.domains` and their subdomains, over http or https. Every redirect is checked. |
| Streams | Stream URLs must also be on an allowed domain. |
| Files | No file system access. `import` is disabled; a plugin is a single file. |
| Load time | Network access is blocked while the file first runs. |
| Time | 30 seconds per call. A plugin that stops responding is restarted. |
| Loops | 10 million iterations per loop. |
| Recursion | 400 nested calls; exceeding it is an error, never a crash. |
| Requests | 32 per call, 16 MB per response, 20 seconds each. |

Memory use is not limited yet. Plugins are trusted code: in a later release every plugin will be signed, and Veena will refuse plugins without a valid signature.

---

## Included plugins

| File | Source |
|---|---|
| `archive-org.js` | Internet Archive: free and public-domain audio. Search + audio links. |
| `youtube-music.js` | YouTube Music songs, albums and playlists. Search only; labels tracks `youtube`. |
| `youtube-audio.js` | Audio links for `youtube` tracks (AAC). Tries several YouTube app types in turn. |
| `youtube-tv-audio.js` | Audio links for `youtube` tracks via the TV app type. Runs YouTube's player code to solve its scrambling. |

Test an audio-link brick without playing:

```sh
veena --plugins ./plugins --verbose resolve youtube:<video-id>
veena --plugins ./plugins --verbose resolve --with youtube-tv-audio youtube:<video-id>
```
