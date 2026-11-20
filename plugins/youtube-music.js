// YouTube Music search brick.
//
// Finds songs, albums and playlists through YouTube Music's web API and
// labels them with the `youtube` kind. Playing them needs an audio-link
// brick for `youtube`, such as youtube-audio.js.

export const meta = {
  id: "youtube-music",
  name: "YouTube Music",
  version: "0.2.0",
  api: 1,
  kinds: ["youtube"],
  domains: ["youtube.com"],
};

const BASE = veena.config.baseUrl ?? "https://music.youtube.com";
/** Used only if the current version can't be read from the site. */
const FALLBACK_CLIENT_VERSION = "1.20251110.01.00";
/** Search filter: songs only. */
const SONGS_FILTER = "EgWKAQIIAWoKEAkQBRAKEAMQBA%3D%3D";
const SESSION_CACHE_SECONDS = 6 * 3600;

export async function search(query) {
  const terms = query.trim();
  if (!terms) return [];
  const filter = veena.config.searchParams ?? SONGS_FILTER;
  let tracks = await searchWith(terms, filter);
  // In some regions the songs-only filter finds nothing; search everything.
  if (tracks.length === 0 && filter) {
    veena.log("no songs found; searching without the songs filter");
    tracks = await searchWith(terms, undefined);
  }
  return tracks;
}

async function searchWith(terms, params) {
  const data = await api("search", { query: terms, ...(params ? { params } : {}) });
  const tracks = collectTracks(data);
  if (tracks.length === 0) {
    if (!hasNoResultsMessage(data)) throw new Error(`could not read search results (${describe(data)})`);
    veena.log(`no results: ${messageText(data) ?? describe(data)}`);
  }
  return tracks;
}

/** An album (`MPREb_...`) or playlist (`PL...`, `OLAK5uy_...`, `VL...`). */
export async function playlist(id) {
  const browseId = /^(PL|OLAK|RD|LL)/.test(id) ? `VL${id}` : id;
  const data = await api("browse", { browseId });
  const tracks = collectTracks(data);
  if (tracks.length === 0) throw new Error(`could not read playlist ${id} (${describe(data)})`);
  return tracks;
}

// ---------------------------------------------------------------- API

async function api(endpoint, body) {
  const session = await getSession();
  const res = await veena.http.fetch(`${BASE}/youtubei/v1/${endpoint}?prettyPrint=false`, {
    method: "POST",
    headers: {
      "Content-Type": "application/json",
      Origin: "https://music.youtube.com",
      Referer: "https://music.youtube.com/",
      "X-Goog-Visitor-Id": session.visitorData ?? "",
      "X-YouTube-Client-Name": "67",
      "X-YouTube-Client-Version": session.clientVersion,
    },
    body: JSON.stringify({
      context: {
        client: {
          clientName: "WEB_REMIX",
          clientVersion: session.clientVersion,
          hl: veena.config.language ?? "en",
          gl: veena.config.region ?? "US",
          visitorData: session.visitorData,
        },
      },
      ...body,
    }),
  });
  if (!res.ok) throw new Error(`${endpoint} request failed with status ${res.status}`);
  return res.json();
}

/** Client version and visitor id, read from the site and cached. */
async function getSession() {
  const cached = veena.cache.get("session");
  if (cached) return JSON.parse(cached);

  let session = { clientVersion: veena.config.clientVersion ?? FALLBACK_CLIENT_VERSION };
  try {
    const res = await veena.http.fetch(`${BASE}/`, {
      // Skips the cookie consent page shown in some regions.
      headers: { Cookie: "SOCS=CAI", "Accept-Language": "en" },
    });
    const version = /"INNERTUBE_CLIENT_VERSION":"([^"]+)"/.exec(res.text);
    const visitor = /"VISITOR_DATA":"([^"]+)"/.exec(res.text);
    if (version && !veena.config.clientVersion) session.clientVersion = version[1];
    if (visitor) session.visitorData = visitor[1];
  } catch (e) {
    veena.log(`using fallback client version: ${e}`);
  }
  veena.cache.set("session", JSON.stringify(session), SESSION_CACHE_SECONDS);
  return session;
}

// ---------------------------------------------------------------- Parsing
//
// Rather than following one exact path through the response, which changes
// often, find every song row anywhere in it.

function collectTracks(data) {
  const tracks = [];
  const seen = new Set();
  walk(data, (node) => {
    const row = node.musicResponsiveListItemRenderer;
    if (!row) return;
    const track = parseRow(row);
    if (track && !seen.has(track.id)) {
      seen.add(track.id);
      tracks.push(track);
    }
  });
  return tracks;
}

function parseRow(row) {
  const videoId =
    row.playlistItemData?.videoId ??
    find(row.overlay, (n) => n.watchEndpoint?.videoId)?.watchEndpoint.videoId ??
    find(row.flexColumns, (n) => n.watchEndpoint?.videoId)?.watchEndpoint.videoId;
  if (!videoId) return null;

  const columns = (row.flexColumns ?? []).map(
    (c) => c.musicResponsiveListItemFlexColumnRenderer?.text?.runs ?? [],
  );
  const title = columns[0]?.map((r) => r.text).join("") || null;
  if (!title) return null;

  const details = columns.slice(1).flat();
  const fixed = (row.fixedColumns ?? []).flatMap(
    (c) => c.musicResponsiveListItemFixedColumnRenderer?.text?.runs ?? [],
  );
  const artists = [];
  let album;
  let duration;
  for (const run of [...details, ...fixed]) {
    const text = run.text?.trim();
    if (!text || text === "•" || text === "&" || text === ",") continue;
    const page = run.navigationEndpoint?.browseEndpoint?.browseEndpointContextSupportedConfigs
      ?.browseEndpointContextMusicConfig?.pageType;
    if (page === "MUSIC_PAGE_TYPE_ARTIST" || page === "MUSIC_PAGE_TYPE_USER_CHANNEL") artists.push(text);
    else if (page === "MUSIC_PAGE_TYPE_ALBUM") album = text;
    else if (/^\d{1,2}(:\d{2}){1,2}$/.test(text)) duration = text;
  }
  // Rows without links: the first plain detail is usually the artist.
  if (artists.length === 0) {
    const plain = details.map((r) => r.text?.trim()).find((t) => t && t !== "•" && !/^\d/.test(t) && t !== "Song");
    if (plain) artists.push(plain);
  }

  const thumbnails =
    find(row.thumbnail, (n) => Array.isArray(n.thumbnails))?.thumbnails ?? [];
  return {
    id: videoId,
    title,
    artist: artists.join(", ") || undefined,
    album,
    durationMs: duration ? toSeconds(duration) * 1000 : undefined,
    artwork: thumbnails.at(-1)?.url,
  };
}

/** The text of the first message YouTube shows instead of results. */
function messageText(data) {
  const message = find(data, (n) => n.messageRenderer)?.messageRenderer;
  const runs = find(message, (n) => Array.isArray(n.runs))?.runs;
  return runs?.map((r) => r.text).join("");
}

function hasNoResultsMessage(data) {
  return find(data, (n) => n.itemSectionRenderer || n.messageRenderer) !== undefined;
}

// ---------------------------------------------------------------- Helpers

/** Call `visit` on every object in a JSON tree. */
function walk(node, visit) {
  if (Array.isArray(node)) {
    for (const item of node) walk(item, visit);
  } else if (node && typeof node === "object") {
    visit(node);
    for (const key in node) walk(node[key], visit);
  }
}

/** First object in a JSON tree matching `test`. */
function find(node, test) {
  let found;
  walk(node, (n) => {
    if (found === undefined && test(n)) found = n;
  });
  return found;
}

function toSeconds(clock) {
  return clock.split(":").reduce((total, part) => total * 60 + Number(part), 0);
}

/** A short outline of a response, to help diagnose layout changes. */
function describe(data) {
  if (!data || typeof data !== "object") return String(data);
  const error = data.error?.message;
  if (error) return `error: ${error}`;
  return `keys: ${Object.keys(data).slice(0, 8).join(", ")}`;
}
