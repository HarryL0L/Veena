// Internet Archive: free and public-domain audio from archive.org.
//
// A search result is an archive item (an album, a concert, a recording).
// Playing it plays the item's first audio file; `playlist(itemId)` lists
// every file in the item as its own track.

export const meta = {
  id: "archive-org",
  name: "Internet Archive",
  version: "1.0.0",
  api: 1,
  kinds: ["archive-org"],
  domains: ["archive.org"],
};

const BASE = "https://archive.org";
/** Preferred audio formats, best first. Opus is skipped: Veena can't decode it yet. */
const FORMATS = ["VBR MP3", "MP3", "128Kbps MP3", "Ogg Vorbis", "Flac", "M4A"];
const MIME = { mp3: "audio/mpeg", ogg: "audio/ogg", flac: "audio/flac", m4a: "audio/mp4" };
const CACHE_SECONDS = 3600;

export async function search(query) {
  const terms = query.trim();
  if (!terms) return [];
  const params = [
    ["q", `(${terms}) AND mediatype:audio`],
    ["rows", String(veena.config.rows ?? 25)],
    ["output", "json"],
    ...["identifier", "title", "creator", "date"].map((field) => ["fl[]", field]),
  ];
  const res = await veena.http.fetch(`${BASE}/advancedsearch.php?${queryString(params)}`);
  if (!res.ok) throw new Error(`search failed with status ${res.status}`);
  return res.json().response.docs.map((doc) => ({
    id: doc.identifier,
    title: text(doc.title) ?? doc.identifier,
    artist: text(doc.creator),
    album: doc.date ? String(doc.date).slice(0, 4) : undefined,
    artwork: `${BASE}/services/img/${doc.identifier}`,
  }));
}

export async function resolve(track) {
  // Ids are either "item" or "item/file name".
  const slash = track.id.indexOf("/");
  const item = slash < 0 ? track.id : track.id.slice(0, slash);
  const files = await audioFiles(item);
  const file = slash < 0 ? files[0] : files.find((f) => f.name === track.id.slice(slash + 1));
  if (!file) throw new Error(`no playable audio in ${item}`);
  return [{ url: downloadUrl(item, file.name), mime: mimeOf(file.name) }];
}

export async function playlist(item) {
  const files = await audioFiles(item);
  return files.map((file) => ({
    id: `${item}/${file.name}`,
    title: file.title || file.name.replace(/\.[^.]+$/, ""),
    artist: file.creator || file.artist,
    album: file.album,
    durationMs: file.length ? Math.round(parseLength(file.length) * 1000) : undefined,
    artwork: `${BASE}/services/img/${item}`,
  }));
}

/** The item's audio files in one format (the best available), in order. */
async function audioFiles(item) {
  const key = `files:${item}`;
  const cached = veena.cache.get(key);
  if (cached) return JSON.parse(cached);

  const res = await veena.http.fetch(`${BASE}/metadata/${encodeURIComponent(item)}`);
  if (!res.ok) throw new Error(`metadata request failed with status ${res.status}`);
  const files = res.json().files ?? [];
  const format = FORMATS.find((f) => files.some((file) => file.format === f));
  const chosen = files
    .filter((file) => file.format === format && file.source !== "metadata")
    .sort((a, b) => (Number(a.track) || 0) - (Number(b.track) || 0) || a.name.localeCompare(b.name));

  veena.cache.set(key, JSON.stringify(chosen), CACHE_SECONDS);
  return chosen;
}

function queryString(pairs) {
  return pairs.map(([k, v]) => `${encodeURIComponent(k)}=${encodeURIComponent(v)}`).join("&");
}

function downloadUrl(item, name) {
  const path = name.split("/").map(encodeURIComponent).join("/");
  return `${BASE}/download/${encodeURIComponent(item)}/${path}`;
}

function mimeOf(name) {
  return MIME[name.split(".").pop().toLowerCase()];
}

/** Lengths are seconds ("1798.97") or clock time ("29:58"). */
function parseLength(length) {
  return String(length)
    .split(":")
    .reduce((total, part) => total * 60 + Number(part), 0);
}

/** Archive fields may be a string or a list of strings. */
function text(value) {
  if (Array.isArray(value)) value = value[0];
  return value ? String(value) : undefined;
}
