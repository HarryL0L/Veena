// Lyrics brick using LRCLIB (lrclib.net), a free, open lyrics database.
//
// Finds synced lyrics (each line with the time it is sung) when there are
// some, or plain lyrics otherwise. YouTube titles often carry extras like
// "(Official Video)" or "Artist - Title"; those are cleaned up first.

export const meta = {
  id: "lrclib",
  name: "LRCLIB",
  version: "1.2.0",
  api: 1,
  domains: ["lrclib.net"],
};

const BASE = veena.config.baseUrl ?? "https://lrclib.net";
/** How far a match's length may be from the song's, in seconds. */
const LENGTH_TOLERANCE = 4;

export async function lyrics(track) {
  const seconds = track.durationMs ? Math.round(track.durationMs / 1000) : undefined;
  // Re-uploads often put "Artist - Title" in the title and their own channel
  // name in the artist field; try that reading too.
  for (const guess of guesses(track.title, track.artist)) {
    const found = await find(guess.title, guess.artist, track.album, seconds);
    if (found) return found;
  }
  return null;
}

function guesses(rawTitle, rawArtist) {
  const first = clean(rawTitle, rawArtist);
  const list = [first];
  const dash = first.title.match(/^(.+?)\s+[-–—]\s+(.+)$/);
  if (dash) list.push({ title: dash[2].trim(), artist: dash[1].trim() });
  return list.filter((g) => g.title);
}

async function find(title, artist, album, seconds) {
  // An exact lookup first: fast, and right when the names match.
  if (artist) {
    const params = new URLSearchParamsLite({ track_name: title, artist_name: artist });
    if (album) params.set("album_name", album);
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

/**
 * "[01:02.34] words" lines into { timeMs, text, words }, in time order.
 * Enhanced LRC marks each word too: "[00:18.89] <00:18.89>We're <00:19.25>no".
 */
function parseLrc(text) {
  const lines = [];
  for (const raw of text.split("\n")) {
    const stamps = [...raw.matchAll(/\[(\d+):(\d+(?:\.\d+)?)\]/g)];
    if (stamps.length === 0) continue;
    const body = raw.replace(/\[[^\]]*\]/g, "");
    const words = parseWords(body);
    const plain = body.replace(/<\d+:\d+(?:\.\d+)?>/g, "").replace(/\s+/g, " ").trim();
    for (const s of stamps) {
      lines.push({ timeMs: toMs(s[1], s[2]), text: plain, ...(words.length ? { words } : {}) });
    }
  }
  return lines.sort((a, b) => a.timeMs - b.timeMs);
}

/** Words after <mm:ss.xx> marks, each keeping the space that follows it. */
function parseWords(body) {
  const marks = [...body.matchAll(/<(\d+):(\d+(?:\.\d+)?)>/g)];
  const words = [];
  marks.forEach((m, i) => {
    const start = m.index + m[0].length;
    const end = i + 1 < marks.length ? marks[i + 1].index : body.length;
    const text = body.slice(start, end).replace(/\s+/g, " ");
    if (text.trim()) words.push({ timeMs: toMs(m[1], m[2]), text: text.trimStart() });
  });
  return words;
}

function toMs(minutes, seconds) {
  return Math.round((Number(minutes) * 60 + Number(seconds)) * 1000);
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
