// Lyrics brick using LRCLIB (lrclib.net), a free, open lyrics database.
//
// Finds synced lyrics (each line with the time it is sung) when there are
// some, or plain lyrics otherwise. YouTube titles often carry extras like
// "(Official Video)" or "Artist - Title"; those are cleaned up first.

export const meta = {
  id: "lrclib",
  name: "LRCLIB",
  version: "1.0.0",
  api: 1,
  domains: ["lrclib.net"],
};

const BASE = veena.config.baseUrl ?? "https://lrclib.net";
/** How far a match's length may be from the song's, in seconds. */
const LENGTH_TOLERANCE = 4;

export async function lyrics(track) {
  const { title, artist } = clean(track.title, track.artist);
  if (!title) return null;
  const seconds = track.durationMs ? Math.round(track.durationMs / 1000) : undefined;

  // An exact lookup first: fast, and right when the names match.
  if (artist) {
    const params = new URLSearchParamsLite({ track_name: title, artist_name: artist });
    if (track.album) params.set("album_name", track.album);
    if (seconds) params.set("duration", String(seconds));
    const res = await veena.http.fetch(`${BASE}/api/get?${params}`);
    if (res.ok) {
      const found = toLyrics(res.json());
      if (found) return found;
    }
  }

  // Otherwise search and pick the closest in length, preferring synced lyrics.
  const query = new URLSearchParamsLite(artist ? { track_name: title, artist_name: artist } : { q: title });
  let res = await veena.http.fetch(`${BASE}/api/search?${query}`);
  let results = res.ok ? res.json() : [];
  if (results.length === 0 && artist) {
    res = await veena.http.fetch(`${BASE}/api/search?${new URLSearchParamsLite({ q: `${artist} ${title}` })}`);
    results = res.ok ? res.json() : [];
  }
  const candidates = results
    .filter((r) => !seconds || !r.duration || Math.abs(r.duration - seconds) <= LENGTH_TOLERANCE)
    .sort((a, b) => score(b) - score(a));
  for (const candidate of candidates) {
    const found = toLyrics(candidate);
    if (found) return found;
  }
  return null;
}

function score(r) {
  return (r.syncedLyrics ? 2 : 0) + (r.plainLyrics ? 1 : 0);
}

function toLyrics(r) {
  if (!r) return null;
  if (r.instrumental) return { lines: [], instrumental: true };
  if (r.syncedLyrics) {
    const lines = parseLrc(r.syncedLyrics);
    if (lines.length) return { lines };
  }
  if (r.plainLyrics) return { lines: r.plainLyrics.split("\n").map((text) => ({ text })) };
  return null;
}

/** "[01:02.34] words" lines into { timeMs, text }, in time order. */
function parseLrc(text) {
  const lines = [];
  for (const raw of text.split("\n")) {
    const stamps = [...raw.matchAll(/\[(\d+):(\d+(?:\.\d+)?)\]/g)];
    if (stamps.length === 0) continue;
    // Word-by-word timing marks like <00:18.89> aren't needed.
    const words = raw.replace(/\[[^\]]*\]/g, "").replace(/<\d+:\d+(?:\.\d+)?>/g, "").replace(/\s+/g, " ").trim();
    for (const s of stamps) {
      lines.push({ timeMs: Math.round((Number(s[1]) * 60 + Number(s[2])) * 1000), text: words });
    }
  }
  return lines.sort((a, b) => a.timeMs - b.timeMs);
}

/** Strip video extras from a title and find the real artist. */
function clean(title, artist) {
  let t = (title ?? "").trim();
  let a = (artist ?? "").replace(/\s*-\s*Topic$/i, "").replace(/VEVO$/i, "").trim();
  // Bracketed extras: (Official Video), [Lyrics], (Audio), (HD), (Visualizer)...
  t = t.replace(/\s*[\(\[][^\)\]]*(official|video|audio|lyric|visuali[sz]er|\bhd\b|\b4k\b|\bmv\b|remaster|music)[^\)\]]*[\)\]]/gi, "");
  t = t.replace(/\s*\|.*$/, "");
  // "Artist - Title": use the part after the dash when it starts with the artist,
  // or when there is no artist.
  const dash = t.match(/^(.+?)\s+[-–—]\s+(.+)$/);
  if (dash) {
    const [, left, right] = dash;
    if (!a || left.toLowerCase().includes(a.toLowerCase()) || a.toLowerCase().includes(left.toLowerCase())) {
      a = a || left;
      t = right;
    }
  }
  t = t.replace(/\s*[\(\[]?\s*(feat\.?|ft\.?)\s+[^\)\]]*[\)\]]?/i, "").trim();
  return { title: t, artist: a };
}

/** Builds a query string; the engine has no URLSearchParams. */
function URLSearchParamsLite(entries) {
  const map = new Map(Object.entries(entries));
  return {
    set: (k, v) => map.set(k, v),
    toString: () => [...map].map(([k, v]) => `${encodeURIComponent(k)}=${encodeURIComponent(v)}`).join("&"),
  };
}
