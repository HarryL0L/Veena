// YouTube audio-link brick.
//
// Turns a `youtube` track (a video id) into a direct audio link. It asks
// YouTube's player API as several app types in turn, because YouTube
// treats them differently and changes which ones work. The client list can
// be replaced through `veena.config.clients` without a new brick.
//
// Only AAC (audio/mp4) is returned: Veena can't decode Opus yet.

export const meta = {
  id: "youtube-audio",
  name: "YouTube audio",
  version: "0.4.0",
  api: 1,
  kinds: ["youtube"],
  domains: ["youtube.com", "googlevideo.com"],
};

const BASE = veena.config.baseUrl ?? "https://www.youtube.com";

/** An Oculus Quest YouTube app of a given version. */
function questApp(version) {
  return {
    name: "ANDROID_VR",
    id: 28,
    version,
    userAgent: `com.google.android.apps.youtube.vr.oculus/${version} (Linux; U; Android 12L; eureka-user Build/SQ3A.220605.009.A1) gzip`,
    extra: { deviceMake: "Oculus", deviceModel: "Quest 3", androidSdkVersion: 32, osName: "Android", osVersion: "12L" },
  };
}

/**
 * App types to ask as, in order. Each returns direct links when it works.
 * Newer app versions first: YouTube distrusts old ones over time.
 */
const DEFAULT_CLIENTS = [
  questApp("1.65.10"),
  questApp("1.62.27"),
  questApp("1.60.19"),
  {
    // Links from this app type often need a token past the first megabyte;
    // the download check below catches that.
    name: "IOS",
    id: 5,
    version: "20.10.4",
    userAgent: "com.google.ios.youtube/20.10.4 (iPhone16,2; U; CPU iOS 18_3_2 like Mac OS X;)",
    extra: { deviceMake: "Apple", deviceModel: "iPhone16,2", osName: "iPhone", osVersion: "18.3.2.22D82" },
  },
];

export async function resolve(track) {
  const clients = veena.config.clients ?? DEFAULT_CLIENTS;
  const problems = [];
  for (const client of clients) {
    const label = `${client.name} ${client.version}`;
    try {
      const streams = await streamsFrom(client, track.id);
      // YouTube can hand out links it then refuses to serve, sometimes only
      // after the first megabyte, so check the start and the end.
      const failure = await probe(streams[0]);
      if (!failure) {
        veena.log(`${label}: links work`);
        return streams.map(({ size, ...stream }) => stream);
      }
      throw new Error(`links refused when downloading ${failure}`);
    } catch (e) {
      const problem = `${label}: ${e.message ?? e}`;
      veena.log(problem);
      problems.push(problem);
    }
  }
  throw new Error(problems.join("; "));
}

/**
 * Download two bytes at the start of a stream and, when its size is known,
 * two near the end. Returns a description of the first failure, or nothing.
 */
async function probe(stream) {
  const offsets = [0];
  if (stream.size > 2_000_000) offsets.push(stream.size - 2);
  for (const offset of offsets) {
    const res = await veena.http.fetch(stream.url, {
      headers: { ...stream.headers, Range: `bytes=${offset}-${offset + 1}` },
    });
    if (res.status !== 200 && res.status !== 206) {
      return `(status ${res.status} at ${offset === 0 ? "the start" : "the end"})`;
    }
  }
  return undefined;
}

/**
 * YouTube's anonymous visitor id. Without it some app types answer
 * "Sign in to confirm you're not a bot".
 */
async function visitorData() {
  const cached = veena.cache.get("visitor");
  if (cached) return cached;
  try {
    const res = await veena.http.fetch(`${BASE}/`, {
      // Skips the cookie consent page shown in some regions.
      headers: { Cookie: "SOCS=CAI", "Accept-Language": "en" },
    });
    const match = /"VISITOR_DATA":"([^"]+)"/.exec(res.text);
    if (match) {
      veena.log("visitor id found");
      veena.cache.set("visitor", match[1], 6 * 3600);
      return match[1];
    }
    veena.log("no visitor id on the YouTube home page");
  } catch (e) {
    veena.log(`could not get a visitor id: ${e}`);
  }
  return undefined;
}

async function streamsFrom(client, videoId) {
  const visitor = await visitorData();
  const res = await veena.http.fetch(`${BASE}/youtubei/v1/player?prettyPrint=false`, {
    method: "POST",
    headers: {
      "Content-Type": "application/json",
      "User-Agent": client.userAgent,
      "X-YouTube-Client-Name": String(client.id),
      "X-YouTube-Client-Version": client.version,
      ...(visitor ? { "X-Goog-Visitor-Id": visitor } : {}),
    },
    body: JSON.stringify({
      context: {
        client: {
          clientName: client.name,
          clientVersion: client.version,
          hl: "en",
          gl: veena.config.region ?? "US",
          visitorData: visitor,
          ...client.extra,
        },
      },
      videoId,
      contentCheckOk: true,
      racyCheckOk: true,
    }),
  });
  if (!res.ok) throw new Error(`player request failed with status ${res.status}`);
  const data = res.json();

  const status = data.playabilityStatus?.status;
  if (status !== "OK") {
    const reason = data.playabilityStatus?.reason ?? data.playabilityStatus?.messages?.[0] ?? "no reason given";
    throw new Error(`${status ?? "unknown status"}: ${reason}`);
  }

  const formats = data.streamingData?.adaptiveFormats ?? [];
  const streams = formats
    .filter((f) => f.url && f.mimeType?.startsWith("audio/mp4"))
    .sort((a, b) => (b.bitrate ?? 0) - (a.bitrate ?? 0))
    .map((f) => ({
      url: f.url,
      mime: "audio/mp4",
      bitrate: f.averageBitrate ?? f.bitrate,
      size: Number(f.contentLength) || undefined,
      // Streams are checked against the app type that requested them.
      headers: { "User-Agent": client.userAgent },
    }));
  if (streams.length === 0) {
    throw new Error(`no AAC audio with a direct link (${summarize(formats)})`);
  }
  return streams;
}

/** e.g. "3 audio/webm with links, 2 audio/mp4 needing signatures". */
function summarize(formats) {
  if (formats.length === 0) return "no formats";
  const counts = {};
  for (const f of formats.filter((f) => f.mimeType?.startsWith("audio/"))) {
    const kind = `${f.mimeType.split(";")[0]} ${f.url ? "with links" : "needing signatures"}`;
    counts[kind] = (counts[kind] ?? 0) + 1;
  }
  const parts = Object.entries(counts).map(([kind, n]) => `${n} ${kind}`);
  return parts.length ? parts.join(", ") : "no audio formats";
}
