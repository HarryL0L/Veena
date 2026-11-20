// YouTube audio-link brick using the TV app type and puzzle solving.
//
// The TV app's links are scrambled: each needs a signature, and an "n"
// value that YouTube checks before serving more than a little data. The
// functions that compute them are in YouTube's player code (base.js),
// which changes and renames everything regularly. This brick runs that
// code in Veena's JavaScript engine and finds the two functions by what
// they do rather than by name.
//
// Only AAC (audio/mp4) is returned: Veena can't decode Opus yet.

export const meta = {
  id: "youtube-tv-audio",
  name: "YouTube audio (TV)",
  version: "0.1.0",
  api: 1,
  kinds: ["youtube"],
  domains: ["youtube.com", "googlevideo.com"],
};

const BASE = veena.config.baseUrl ?? "https://www.youtube.com";
const CLIENT = {
  name: "TVHTML5",
  id: 7,
  version: veena.config.tvClientVersion ?? "7.20250312.16.00",
  userAgent: "Mozilla/5.0 (ChromiumStylePlatform) Cobalt/Version",
};
/** Links without a proof-of-origin token stop working after this point. */
const TOKEN_FREE_LIMIT = 1024 * 1024;

/** The solved player, kept for as long as this brick runs. */
let player;

export async function resolve(track) {
  const solved = await getPlayer();
  const visitor = await visitorData();
  const res = await veena.http.fetch(`${BASE}/youtubei/v1/player?prettyPrint=false`, {
    method: "POST",
    headers: {
      "Content-Type": "application/json",
      "User-Agent": CLIENT.userAgent,
      Origin: "https://www.youtube.com",
      "X-YouTube-Client-Name": String(CLIENT.id),
      "X-YouTube-Client-Version": CLIENT.version,
      ...(visitor ? { "X-Goog-Visitor-Id": visitor } : {}),
    },
    body: JSON.stringify({
      context: {
        client: {
          clientName: CLIENT.name,
          clientVersion: CLIENT.version,
          hl: "en",
          gl: veena.config.region ?? "US",
          visitorData: visitor,
        },
      },
      videoId: track.id,
      playbackContext: {
        contentPlaybackContext: { signatureTimestamp: solved.signatureTimestamp, html5Preference: "HTML5_PREF_WANTS" },
      },
      contentCheckOk: true,
      racyCheckOk: true,
    }),
  });
  if (!res.ok) throw new Error(`player request failed with status ${res.status}`);
  const data = res.json();
  const status = data.playabilityStatus?.status;
  if (status !== "OK") {
    throw new Error(`${status ?? "unknown status"}: ${data.playabilityStatus?.reason ?? "no reason given"}`);
  }

  const formats = (data.streamingData?.adaptiveFormats ?? [])
    .filter((f) => f.mimeType?.startsWith("audio/mp4") && (f.url || f.signatureCipher || f.cipher))
    .sort((a, b) => (b.bitrate ?? 0) - (a.bitrate ?? 0));
  if (formats.length === 0) throw new Error("no AAC audio offered");

  const streams = formats.map((f) => ({
    url: unscramble(f, solved),
    mime: "audio/mp4",
    bitrate: f.averageBitrate ?? f.bitrate,
    size: Number(f.contentLength) || undefined,
    headers: { "User-Agent": CLIENT.userAgent },
  }));
  const failure = await probe(streams[0]);
  if (failure) throw new Error(`links refused when downloading ${failure}`);
  veena.log("TV links work");
  return streams.map(({ size, ...stream }) => stream);
}

// ---------------------------------------------------------------- Links

/** A format's playable URL: signature added and "n" transformed. */
function unscramble(format, solved) {
  let url = format.url;
  const cipher = format.signatureCipher ?? format.cipher;
  if (cipher) {
    const parts = parseQuery(cipher);
    if (!parts.s || !parts.url) throw new Error("unexpected signature data");
    url = `${parts.url}&${parts.sp ?? "signature"}=${encodeURIComponent(solved.signature(parts.s))}`;
  }
  return url.replace(/([?&])n=([^&]*)/, (_, sep, n) => {
    const result = solved.n(decodeURIComponent(n));
    return `${sep}n=${encodeURIComponent(result)}`;
  });
}

async function probe(stream) {
  const checks = [[0, "at the start"]];
  if (stream.size > TOKEN_FREE_LIMIT + 2) {
    checks.push([TOKEN_FREE_LIMIT, "past the first megabyte; YouTube wants a proof-of-origin token"]);
    checks.push([stream.size - 2, "at the end"]);
  }
  for (const [offset, where] of checks) {
    const res = await veena.http.fetch(stream.url, {
      headers: { ...stream.headers, Range: `bytes=${offset}-${offset + 1}` },
    });
    if (res.status !== 200 && res.status !== 206) return `(status ${res.status} ${where})`;
  }
  return undefined;
}

// ---------------------------------------------------------------- Player code

/** Download base.js for the current player and find its functions. */
async function getPlayer() {
  const id = await playerId();
  if (player?.id === id) return player;

  const url = `${BASE}/s/player/${id}/player_ias.vflset/en_US/base.js`;
  const res = await veena.http.fetch(url);
  if (!res.ok) throw new Error(`player code download failed with status ${res.status}`);
  const code = res.text;
  veena.log(`player ${id}: ${Math.round(code.length / 1024)} KB`);

  const sts = /(?:signatureTimestamp|sts)\s*:\s*(\d+)/.exec(code);
  if (!sts) throw new Error(`player ${id}: no signature timestamp found`);

  const started = Date.now();
  const lookup = load(code);
  veena.log(`player ${id}: ran in ${Date.now() - started} ms`);

  const candidates = findFunctions(code);
  const signature = pick(candidates.signature, lookup, isSignatureFunction);
  const n = pick(candidates.n, lookup, isNFunction);
  veena.log(
    `player ${id}: ${candidates.signature.length} signature and ${candidates.n.length} n candidates; ` +
      `chose ${signature?.name ?? "none"} and ${n?.name ?? "none"}`,
  );
  if (!signature) throw new Error(`player ${id}: signature function not found`);
  if (!n) throw new Error(`player ${id}: n function not found`);

  player = { id, signatureTimestamp: Number(sts[1]), signature: signature.fn, n: wrapN(n.fn) };
  return player;
}

async function playerId() {
  const res = await veena.http.fetch(`${BASE}/iframe_api`);
  const match = /player\\?\/([0-9a-fA-F]{8})\\?\//.exec(res.text);
  if (!match) throw new Error("could not find the current player version");
  return match[1];
}

/**
 * Run the player code with a stand-in browser environment, and return a
 * function that looks up any name inside the player's private scope.
 */
function load(code) {
  // The player's code is one function, `(function(g){...})(_yt_player)`.
  // A line added at its start can later read its private names via eval.
  const start = /\(function\(([\w$]+)\)\{/.exec(code);
  if (!start) throw new Error("unexpected player code layout");
  const at = start.index + start[0].length;
  const hooked = `${code.slice(0, at)}globalThis.__veenaLookup=function(n){return eval(n)};${code.slice(at)}`;

  installBrowserStandIns();
  try {
    (0, eval)(hooked);
  } catch (e) {
    // Expected: setup code that needs a real page fails partway, after the
    // functions we need are defined.
    veena.log(`player setup stopped early: ${String(e).slice(0, 120)}`);
  }
  const lookup = globalThis.__veenaLookup;
  if (typeof lookup !== "function") throw new Error("could not look inside the player code");
  return lookup;
}

/** Objects that accept any use, standing in for browser APIs. */
function installBrowserStandIns() {
  const anything = () => {
    const target = function () {};
    return new Proxy(target, {
      get: (_, key) => (key === Symbol.toPrimitive ? () => "" : key === "length" ? 0 : anything()),
      set: () => true,
      has: () => true,
      apply: () => anything(),
      construct: () => anything(),
    });
  };
  const location = {
    href: "https://www.youtube.com/tv",
    origin: "https://www.youtube.com",
    protocol: "https:",
    host: "www.youtube.com",
    hostname: "www.youtube.com",
    pathname: "/tv",
    search: "",
    hash: "",
  };
  const standIns = {
    document: anything(),
    navigator: { userAgent: CLIENT.userAgent, language: "en", platform: "" },
    location,
    localStorage: anything(),
    sessionStorage: anything(),
    XMLHttpRequest: anything(),
    setTimeout: () => 0,
    clearTimeout: () => {},
    setInterval: () => 0,
    clearInterval: () => {},
    requestAnimationFrame: () => 0,
    addEventListener: () => {},
    removeEventListener: () => {},
  };
  for (const [name, value] of Object.entries(standIns)) {
    if (!(name in globalThis)) globalThis[name] = value;
  }
  if (!("window" in globalThis)) globalThis.window = globalThis;
  if (!("self" in globalThis)) globalThis.self = globalThis;
}

/**
 * Names of one-argument functions in the player code whose source looks
 * like a signature function (short, splits and joins characters) or an n
 * function (long, splits and joins).
 */
function findFunctions(code) {
  const signature = [];
  const n = [];
  const pattern = /(?:^|[;,{}\s(])([A-Za-z_$][\w$]*)\s*=\s*function\(\s*[\w$]+\s*\)\s*\{|function\s+([A-Za-z_$][\w$]*)\(\s*[\w$]+\s*\)\s*\{/g;
  for (const match of code.matchAll(pattern)) {
    const name = match[1] ?? match[2];
    const body = bodyAt(code, match.index + match[0].length - 1);
    if (!body.includes("split") || !body.includes("join")) continue;
    if (body.length < 3000) signature.push(name);
    if (body.length > 600) n.push(name);
  }
  return { signature, n };
}

/** The function body starting at the `{` at `open`, by brace matching. */
function bodyAt(code, open) {
  let depth = 0;
  let quote = null;
  const limit = Math.min(code.length, open + 200000);
  for (let i = open; i < limit; i++) {
    const c = code[i];
    if (quote) {
      if (c === "\\") i++;
      else if (c === quote) quote = null;
    } else if (c === '"' || c === "'" || c === "`") quote = c;
    else if (c === "{") depth++;
    else if (c === "}" && --depth === 0) return code.slice(open, i + 1);
  }
  return code.slice(open, limit);
}

/** The first candidate that behaves as `test` expects. */
function pick(names, lookup, test) {
  for (const name of names) {
    let fn;
    try {
      fn = lookup(name);
    } catch {
      continue;
    }
    if (typeof fn !== "function") continue;
    try {
      if (test(fn)) return { name, fn };
    } catch {
      // Not it.
    }
  }
  return undefined;
}

const SAMPLE_SIGNATURE = "AOq0QJ8wRAIgXmPlOPSBkkUs1bYFYlJCfe29xxLTnR1o6ZK7Vb1m1q4CIGR5Iu0ADBc0bMgDGNIq6GvdhZkTHvXlrrjZOq9Ki7Vf";
const SAMPLE_N = "a1Bc2De3Fg4Hi5Jk";

/** Shuffles a long string into a slightly shorter one of the same characters. */
function isSignatureFunction(fn) {
  const out = fn(SAMPLE_SIGNATURE);
  if (typeof out !== "string" || out === SAMPLE_SIGNATURE) return false;
  if (out.length < 80 || out.length > SAMPLE_SIGNATURE.length) return false;
  const pool = {};
  for (const c of SAMPLE_SIGNATURE) pool[c] = (pool[c] ?? 0) + 1;
  for (const c of out) if (!pool[c]--) return false;
  return fn(SAMPLE_SIGNATURE) === out;
}

/** Turns a short code into a different short code, the same way each time. */
function isNFunction(fn) {
  const out = fn(SAMPLE_N);
  if (typeof out !== "string" || out === SAMPLE_N) return false;
  if (out.length < 8 || out.length > 40 || out.startsWith("enhanced_except")) return false;
  return fn(SAMPLE_N) === out;
}

/** The n function reports failure by returning an error string. */
function wrapN(fn) {
  return (n) => {
    const out = fn(n);
    if (typeof out !== "string" || out.startsWith("enhanced_except")) throw new Error("n function failed");
    return out;
  };
}

// ---------------------------------------------------------------- Helpers

async function visitorData() {
  const cached = veena.cache.get("visitor");
  if (cached) return cached;
  try {
    const res = await veena.http.fetch(`${BASE}/`, { headers: { Cookie: "SOCS=CAI", "Accept-Language": "en" } });
    const match = /"VISITOR_DATA":"([^"]+)"/.exec(res.text);
    if (match) {
      veena.cache.set("visitor", match[1], 6 * 3600);
      return match[1];
    }
  } catch (e) {
    veena.log(`could not get a visitor id: ${e}`);
  }
  return undefined;
}

function parseQuery(query) {
  const out = {};
  for (const pair of query.split("&")) {
    const at = pair.indexOf("=");
    if (at < 0) continue;
    const decode = (s) => decodeURIComponent(s.replace(/\+/g, " "));
    out[decode(pair.slice(0, at))] = decode(pair.slice(at + 1));
  }
  return out;
}
