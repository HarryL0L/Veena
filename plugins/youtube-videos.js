// YouTube video search brick.
//
// Finds ordinary YouTube videos (uploads, live versions, covers, lyric
// videos) and labels them with the `youtube` kind, so the same audio-link
// bricks play them. The Android app lists these under "Videos" and YouTube
// Music results under "Music".

export const meta = {
  id: "youtube-videos",
  name: "YouTube videos",
  version: "1.0.0",
  api: 1,
  kinds: ["youtube"],
  domains: ["youtube.com"],
};

const BASE = veena.config.baseUrl ?? "https://www.youtube.com";
/** Used only if the current version can't be read from the site. */
const FALLBACK_CLIENT_VERSION = "2.20251119.01.00";
/** Search filter: videos only (no channels, playlists or shorts shelves). */
const VIDEOS_FILTER = "EgIQAQ%3D%3D";
const SESSION_CACHE_SECONDS = 6 * 3600;

export async function search(query) {
  const terms = query.trim();
  if (!terms) return [];
  const session = await getSession();
  const res = await veena.http.fetch(`${BASE}/youtubei/v1/search?prettyPrint=false`, {
    method: "POST",
    headers: {
      "Content-Type": "application/json",
      "X-YouTube-Client-Name": "1",
      "X-YouTube-Client-Version": session.clientVersion,
      ...(session.visitorData ? { "X-Goog-Visitor-Id": session.visitorData } : {}),
    },
    body: JSON.stringify({
      context: {
        client: {
          clientName: "WEB",
          clientVersion: session.clientVersion,
          hl: veena.config.language ?? "en",
          gl: veena.config.region ?? "US",
          visitorData: session.visitorData,
        },
      },
      query: terms,
      params: veena.config.searchParams ?? VIDEOS_FILTER,
    }),
  });
  if (!res.ok) throw new Error(`search request failed with status ${res.status}`);

  const tracks = [];
  const seen = new Set();
  walk(res.json(), (node) => {
    const v = node.videoRenderer;
    if (!v?.videoId || seen.has(v.videoId)) return;
    const length = v.lengthText?.simpleText;
    // No length means a live stream or an upcoming premiere: not playable.
    if (!length) return;
    seen.add(v.videoId);
    tracks.push({
      id: v.videoId,
      title: runsText(v.title) ?? v.videoId,
      artist: runsText(v.ownerText) ?? runsText(v.longBylineText),
      durationMs: toSeconds(length) * 1000,
      artwork: v.thumbnail?.thumbnails?.at(-1)?.url,
    });
  });
  return tracks;
}

/** Client version and visitor id, read from the site and cached. */
async function getSession() {
  const cached = veena.cache.get("session");
  if (cached) return JSON.parse(cached);
  const session = { clientVersion: veena.config.clientVersion ?? FALLBACK_CLIENT_VERSION };
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

function runsText(text) {
  if (!text) return undefined;
  if (text.simpleText) return text.simpleText;
  const joined = (text.runs ?? []).map((r) => r.text).join("");
  return joined || undefined;
}

/** Call `visit` on every object in a JSON tree. */
function walk(node, visit) {
  if (Array.isArray(node)) {
    for (const item of node) walk(item, visit);
  } else if (node && typeof node === "object") {
    visit(node);
    for (const key in node) walk(node[key], visit);
  }
}

function toSeconds(clock) {
  return clock.split(":").reduce((total, part) => total * 60 + Number(part), 0);
}
