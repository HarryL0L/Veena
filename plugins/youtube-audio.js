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
  version: "0.1.0",
  api: 1,
  kinds: ["youtube"],
  domains: ["youtube.com", "googlevideo.com"],
};

const BASE = veena.config.baseUrl ?? "https://www.youtube.com";

/** App types to ask as, in order. Each returns direct links when it works. */
const DEFAULT_CLIENTS = [
  {
    name: "ANDROID_VR",
    id: 28,
    version: "1.60.19",
    userAgent:
      "com.google.android.apps.youtube.vr.oculus/1.60.19 (Linux; U; Android 12L; eureka-user Build/SQ3A.220605.009.A1) gzip",
    extra: { deviceMake: "Oculus", deviceModel: "Quest 3", androidSdkVersion: 32, osName: "Android", osVersion: "12L" },
  },
  {
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
    try {
      const streams = await streamsFrom(client, track.id);
      if (streams.length > 0) return streams;
      problems.push(`${client.name}: no AAC audio with a direct link`);
    } catch (e) {
      problems.push(`${client.name}: ${e.message ?? e}`);
    }
  }
  throw new Error(problems.join("; "));
}

async function streamsFrom(client, videoId) {
  const res = await veena.http.fetch(`${BASE}/youtubei/v1/player?prettyPrint=false`, {
    method: "POST",
    headers: {
      "Content-Type": "application/json",
      "User-Agent": client.userAgent,
      "X-YouTube-Client-Name": String(client.id),
      "X-YouTube-Client-Version": client.version,
    },
    body: JSON.stringify({
      context: {
        client: {
          clientName: client.name,
          clientVersion: client.version,
          hl: "en",
          gl: veena.config.region ?? "US",
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
  return formats
    .filter((f) => f.url && f.mimeType?.startsWith("audio/mp4"))
    .sort((a, b) => (b.bitrate ?? 0) - (a.bitrate ?? 0))
    .map((f) => ({
      url: f.url,
      mime: "audio/mp4",
      bitrate: f.averageBitrate ?? f.bitrate,
      // Streams are checked against the app type that requested them.
      headers: { "User-Agent": client.userAgent },
    }));
}
