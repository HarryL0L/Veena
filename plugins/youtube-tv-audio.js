// YouTube audio-link brick using the TV app type and puzzle solving.
//
// The TV app's links are scrambled: each needs a signature, and an "n"
// value that YouTube checks before serving more than a little data. The
// functions that compute them are in YouTube's player code (base.js),
// which changes and renames everything regularly. This brick runs that
// code in Veena's JavaScript engine and finds the two functions by what
// they do rather than by name.
//
// The main method follows yt-dlp's ejs solver (public domain,
// github.com/yt-dlp/ejs): find the player's URL-building function by the
// `X.set("alr","yes")` statement it contains, then let it unscramble a
// test URL. Older function searches remain as a fallback.
//
// Only AAC (audio/mp4) is returned: Veena can't decode Opus yet.

export const meta = {
  id: "youtube-tv-audio",
  name: "YouTube audio (TV)",
  version: "1.0.0",
  api: 1,
  kinds: ["youtube"],
  domains: ["youtube.com", "googlevideo.com"],
  // Tried before youtube-audio, whose links are now cut off after 1 MB.
  priority: 10,
};

const BASE = veena.config.baseUrl ?? "https://www.youtube.com";
/** The simplified TV app: works without a proof-of-origin token. */
const TV_SIMPLY = {
  name: "TVHTML5_SIMPLY",
  id: 75,
  version: "1.0",
  userAgent: "Mozilla/5.0 (ChromiumStylePlatform) Cobalt/Version",
};

/** The TV app. Its version is read from YouTube's TV page when possible. */
const TV = {
  name: "TVHTML5",
  id: 7,
  version: "7.20250312.16.00",
  userAgent: "Mozilla/5.0 (ChromiumStylePlatform) Cobalt/Version",
  page: "/tv",
};

/** The Oculus Quest app; its links carry an n value to transform too. */
const QUEST = {
  name: "ANDROID_VR",
  id: 28,
  version: "1.65.10",
  userAgent:
    "com.google.android.apps.youtube.vr.oculus/1.65.10 (Linux; U; Android 12L; eureka-user Build/SQ3A.220605.009.A1) gzip",
  extra: { deviceMake: "Oculus", deviceModel: "Quest 3", androidSdkVersion: 32, osName: "Android", osVersion: "12L" },
};

/** App types to try, in order; replaceable through `veena.config.tvClients`. */
const CLIENTS = veena.config.tvClients ?? [TV_SIMPLY, TV, QUEST];
/** Links without a proof-of-origin token stop working after this point. */
const TOKEN_FREE_LIMIT = 1024 * 1024;

/** The solved player, kept for as long as this brick runs. */
let player;

export async function resolve(track) {
  const solved = await getPlayer();
  const problems = [];
  for (const client of CLIENTS) {
    let label = `${client.name} ${client.version}`;
    try {
      const current = await withCurrentVersion(client);
      label = `${current.name} ${current.version}`;
      const streams = await streamsFrom(current, track.id, solved);
      const failure = await probe(streams[0]);
      if (failure) throw new Error(`links refused when downloading ${failure}`);
      veena.log(`${label}: links work`);
      return streams.map(({ size, ...stream }) => stream);
    } catch (e) {
      const problem = `${label}: ${e.message ?? e}`;
      veena.log(problem);
      problems.push(problem);
    }
  }
  throw new Error(problems.join("; "));
}

async function streamsFrom(client, videoId, solved) {
  const visitor = await visitorData();
  const res = await veena.http.fetch(`${BASE}/youtubei/v1/player?prettyPrint=false`, {
    method: "POST",
    headers: {
      "Content-Type": "application/json",
      "User-Agent": client.userAgent,
      Origin: "https://www.youtube.com",
      Referer: `https://www.youtube.com${client.page ?? "/"}`,
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
          userAgent: client.userAgent,
          visitorData: visitor,
          ...client.extra,
        },
      },
      videoId,
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
  return formats.map((f) => ({
    url: unscramble(f, solved),
    mime: "audio/mp4",
    bitrate: f.averageBitrate ?? f.bitrate,
    size: Number(f.contentLength) || undefined,
    headers: { "User-Agent": client.userAgent },
  }));
}

/** Use the app version YouTube's own page for that app currently serves. */
async function withCurrentVersion(client) {
  if (!client.page) return client;
  const key = `version:${client.name}`;
  let version = veena.cache.get(key);
  if (!version) {
    try {
      const res = await veena.http.fetch(`${BASE}${client.page}`, {
        headers: { "User-Agent": client.userAgent, Cookie: "SOCS=CAI", "Accept-Language": "en" },
      });
      version = /"INNERTUBE_CLIENT_VERSION":"([^"]+)"/.exec(res.text)?.[1];
      const jsUrl = /"(?:PLAYER_JS_URL|jsUrl)":"([^"]+)"/.exec(res.text)?.[1];
      const sts = /"STS":(\d+)/.exec(res.text)?.[1];
      veena.log(`${client.name}: current version ${version ?? "not found"}; player ${jsUrl ?? "not found"}; STS ${sts ?? "not found"}`);
    } catch (e) {
      veena.log(`${client.name}: could not read the current version: ${e}`);
    }
    if (version) veena.cache.set(key, version, 24 * 3600);
  }
  return version ? { ...client, version } : client;
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
  if (veena.config.debugGrep) debugGrep(code, veena.config.debugGrep, veena.config.debugContext ?? 150);

  const sts = /(?:signatureTimestamp|sts)\s*:\s*(\d+)/.exec(code);
  if (!sts) throw new Error(`player ${id}: no signature timestamp found`);

  const started = Date.now();
  const lookup = load(code);
  veena.log(`player ${id}: ran in ${Date.now() - started} ms`);

  const viaUrl = urlBuilderSolver(code, lookup);
  if (viaUrl) {
    player = { id, signatureTimestamp: Number(sts[1]), ...viaUrl };
    return player;
  }

  const candidates = findFunctions(code);
  const sites = callSites(code);
  const shaped = signatureShaped(code, candidates.signature);
  veena.log(
    `player ${id}: ${candidates.all.length} functions; ` +
      `${sites.signature.length} signature and ${sites.n.length} n call sites; ` +
      `${candidates.signature.length} signature-shaped and ${candidates.n.length} n-shaped functions`,
  );
  for (const c of candidates.signature.slice(0, 6)) {
    veena.log(`signature-shaped: ${c.name}(${c.params.join(",")}) ${c.start}`);
  }
  const byName = (items) => items.map((c) => ({ label: c.name, expr: c.name }));
  const dispatched = dispatcherCalls(code);
  veena.log(`player ${id}: ${dispatched.length} dispatcher calls`);
  const n = pick("n", [...sites.n, ...byName(candidates.n), ...dispatched], lookup, isNFunction, 8000);
  let signature = pick("signature", [...shaped, ...sites.signature, ...dispatched], lookup, isSignatureFunction, 8000);
  // The player fixes a link's n value and its signature in the same place,
  // so the signature function is called near where n is applied.
  if (!signature && n?.index !== undefined) {
    const around = code.slice(Math.max(0, n.index - 700), n.index + 500).replace(/\s+/g, " ");
    veena.log(`code around ${n.label}: ${around}`);
    signature = pick("signature (near n)", callsNear(code, n.index, 1500), lookup, isSignatureFunction, 6000);
  }
  veena.log(`player ${id}: chose ${signature?.label ?? "none"} and ${n?.label ?? "none"}`);
  if (!signature) throw new Error(`player ${id}: signature function not found`);
  if (!n) throw new Error(`player ${id}: n function not found`);

  player = { id, signatureTimestamp: Number(sts[1]), signature: wrapSignature(signature.fn), n: wrapN(n.fn) };
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
    navigator: { userAgent: TV.userAgent, language: "en", platform: "" },
    location,
    // The engine is built without Intl; the player only formats with it.
    Intl: anything(),
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
 * Where the player applies its functions. The signature function is called
 * on `decodeURIComponent(...)` of the scrambled signature, sometimes with
 * extra arguments first: `F(3,decodeURIComponent(s))`. The n function
 * replaces a value with a function of itself: `b=F(b)` or `b=G[0](b)`.
 * Each site becomes an expression for a one-argument wrapper.
 */
function callSites(code) {
  const unique = (items) => [...new Map(items.map((i) => [i.expr, i])).values()];
  const signature = [];
  for (const m of code.matchAll(/([A-Za-z_$][\w$]*(?:\[\d+\])?)\(((?:[\w$.]+,){0,3})decodeURIComponent\(/g)) {
    if (m[1] === "decodeURIComponent" || m[1] === "encodeURIComponent") continue;
    signature.push({ label: `${m[1]}(${m[2]}…)`, expr: `(function(s){return ${m[1]}(${m[2]}s)})` });
  }
  const n = [];
  for (const m of code.matchAll(/(?:^|[^\w$.])([A-Za-z_$][\w$]*)=([A-Za-z_$][\w$]*(?:\[\d+\])?)\(((?:[\w$]+,){0,2})\1\)/g)) {
    n.push({ label: `${m[2]}(${m[3]}…)`, expr: `(function(s){return ${m[2]}(${m[3]}s)})`, index: m.index });
  }
  // Recent players reach the n function through a list: try those first.
  n.sort((a, b) => Number(b.label.includes("[")) - Number(a.label.includes("[")));
  return { signature: unique(signature), n: unique(n) };
}

/**
 * Calls of "dispatcher" functions: newer players fold several jobs into
 * one function whose first two numbers select the job, so the signature
 * and n functions are calls like `F(12,3456,x)`. Each becomes a wrapper
 * `s => F(12,3456,s)`.
 */
function dispatcherCalls(code) {
  const out = new Map();
  for (const m of code.matchAll(/(?:^|[^\w$.])([A-Za-z_$][\w$]*)\((\d+),(\d+),[\w$.]+\)/g)) {
    const expr = `(function(s){return ${m[1]}(${m[2]},${m[3]},s)})`;
    if (!out.has(expr)) out.set(expr, { label: `${m[1]}(${m[2]},${m[3]},…)`, expr });
  }
  return [...out.values()];
}

/**
 * Wrappers for each function called within `radius` characters of
 * `index`, keeping simple leading arguments: `F(3,x)` becomes `F(3,s)`.
 */
function callsNear(code, index, radius) {
  const start = Math.max(0, index - radius);
  const nearby = code.slice(start, index + radius);
  const out = new Map();
  for (const m of nearby.matchAll(/(?:^|[^\w$.])([A-Za-z_$][\w$]*(?:\[\d+\])?)\(((?:[\w$.]+,){0,3})/g)) {
    if (/^(?:if|for|while|switch|catch|function|return|typeof|new)$/.test(m[1])) continue;
    const expr = `(function(s){return ${m[1]}(${m[2]}s)})`;
    if (!out.has(expr)) out.set(expr, { label: `${m[1]}(${m[2]}…)`, expr });
  }
  return [...out.values()];
}

/**
 * One-argument functions in the player code. Signature and n functions are
 * recognised by how they start, which survives renaming and hidden method
 * names: the signature function begins `a=a.split("")` (or `a=a[T[1]](..)`)
 * and the n function begins `var b=a.split(..)` (or `var b=a[T[1]](..)`).
 */
function findFunctions(code) {
  const all = [];
  const signature = [];
  const n = [];
  const header =
    /(?:^|[;,{}\s(])([A-Za-z_$][\w$]*)\s*=\s*function\(\s*([\w$]+(?:\s*,\s*[\w$]+){0,2})\s*\)\s*\{|function\s+([A-Za-z_$][\w$]*)\(\s*([\w$]+(?:\s*,\s*[\w$]+){0,2})\s*\)\s*\{/g;
  // `{a=a.split(` or `{a=a[`: an argument split into itself.
  const splitsSelf = /^\{\s*([\w$]+)\s*=\s*([\w$]+)\s*(?:\.split\b|\[)/;
  // `{var b=a.split(` or `{var b=a[`: an argument split into a variable.
  const splitsIntoVar = /^\{\s*var\s+[\w$]+\s*=\s*([\w$]+)\s*(?:\.split\b|\[)/;
  for (const match of code.matchAll(header)) {
    const name = match[1] ?? match[3];
    const params = (match[2] ?? match[4]).split(",").map((p) => p.trim());
    const last = params[params.length - 1];
    // Measuring up to 3000 characters tells small from large; longer
    // bodies count as large.
    const body = bodyAt(code, match.index + match[0].length - 1, 3000);
    all.push({ name, length: body.length });
    const self = splitsSelf.exec(body);
    const intoVar = splitsIntoVar.exec(body);
    if (body.length < 2500 && self && self[1] === last && self[2] === last) {
      signature.push({ name, params, start: body.slice(0, 70) });
    }
    if (params.length === 1 && body.length > 600 && intoVar && intoVar[1] === last) n.push({ name });
  }
  return { all, signature, n };
}

/**
 * Wrapper expressions for signature-shaped functions. One-argument
 * functions are used directly; for `F(x,a)` the leading arguments are
 * taken from a place the player calls it, e.g. `F(3,`.
 */
function signatureShaped(code, shapes) {
  const out = [];
  for (const { name, params } of shapes) {
    if (params.length === 1) {
      out.push({ label: name, expr: name });
      continue;
    }
    const lead = params.length - 1;
    const escaped = name.replace(/\$/g, "\\$");
    const call = new RegExp(`(?:^|[^\\w$.])${escaped}\\(((?:[\\w$.]+,){${lead}})`, "g");
    const seen = new Set();
    for (const m of code.matchAll(call)) {
      if (seen.has(m[1])) continue;
      seen.add(m[1]);
      out.push({ label: `${name}(${m[1]}…)`, expr: `(function(s){return ${name}(${m[1]}s)})` });
      if (seen.size >= 3) break;
    }
  }
  return out;
}

/** The function body starting at the `{` at `open`, by brace matching. */
function bodyAt(code, open, max) {
  let depth = 0;
  let quote = null;
  const limit = Math.min(code.length, open + max);
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

/**
 * Functions whose body has `X.set("alr","yes");` as a statement of its own.
 * In current players this is the function that builds a stream URL:
 * `F(url, "s", scrambledSignature)` returns a URL object whose signature is
 * already unscrambled, and one more method of that object transforms n.
 */
function urlBuilders(code) {
  const names = [];
  const header = /(?:^|[;,{}\s])([A-Za-z_$][\w$.]*)\s*=\s*function\([^)]*\)\s*\{|function\s+([A-Za-z_$][\w$]*)\([^)]*\)\s*\{/g;
  for (const m of code.matchAll(/[;{]\s*[\w$]+\.[\w$]+\("alr","yes"\);/g)) {
    const before = code.slice(Math.max(0, m.index - 1500), m.index + 1);
    let last;
    for (const h of before.matchAll(header)) last = h;
    if (!last) continue;
    const open = m.index - before.length + 1 + last.index + last[0].length - 1;
    const body = bodyAt(code, open, 20000);
    if (open + body.length > m.index) names.push(last[1] ?? last[2]);
  }
  return [...new Set(names)];
}

/** Unscramble with a URL builder, following yt-dlp's ejs. */
function solveWithUrl(make, sig, n) {
  const url = make("https://youtube.com/watch?v=veena", "s", sig ? encodeURIComponent(sig) : undefined);
  url.set("n", n);
  const proto = Object.getPrototypeOf(url);
  const keys = Object.keys(proto).concat(Object.getOwnPropertyNames(proto));
  for (const key of keys) {
    if (!["constructor", "set", "get", "clone"].includes(key)) {
      url[key]();
      break;
    }
  }
  const s = url.get("s");
  return { sig: s ? decodeURIComponent(s) : null, n: url.get("n") ?? null };
}

/** Signature and n functions from the first URL builder that works. */
function urlBuilderSolver(code, lookup) {
  const builders = urlBuilders(code);
  veena.log(`URL builders: ${builders.join(", ") || "none"}`);
  for (const name of builders) {
    const found = veena.attempt(lookup, name);
    if (!found.ok || typeof found.value !== "function") {
      veena.log(`${name}: not usable (${found.error ?? typeof found.value})`);
      continue;
    }
    const make = found.value;
    const signature = (s) => {
      const out = safely(solveWithUrl, make, s, SAMPLE_N).sig;
      if (typeof out !== "string") throw new Error("signature not unscrambled");
      return out;
    };
    const n = (value) => {
      const out = safely(solveWithUrl, make, undefined, value).n;
      if (typeof out !== "string" || out.startsWith("enhanced_except")) throw new Error("n not transformed");
      return out;
    };
    try {
      const sigVerdict = isSignatureFunction(signature);
      const nVerdict = isNFunction(n);
      if (sigVerdict === true && nVerdict === true) {
        veena.log(`${name}: solves signature and n`);
        return { signature, n };
      }
      veena.log(`${name}: signature ${sigVerdict}, n ${nVerdict}`);
    } catch (e) {
      veena.log(`${name}: failed: ${String(e.message ?? e).slice(0, 100)}`);
    }
  }
  return undefined;
}

/**
 * The first candidate that behaves as `test` expects. Candidates are
 * `{ label, expr }`, where `expr` is evaluated inside the player's scope.
 * Logs why the first few were rejected, to help when YouTube changes.
 */
function pick(kind, candidates, lookup, test, timeLimitMs) {
  const started = Date.now();
  const rejections = [];
  let tested = 0;
  for (const candidate of candidates) {
    if (Date.now() - started > timeLimitMs) {
      veena.log(`${kind}: stopped after ${tested} of ${candidates.length} (time limit)`);
      break;
    }
    tested++;
    let reason;
    const found = veena.attempt(lookup, candidate.expr);
    if (!found.ok) {
      reason = `lookup failed: ${found.error.slice(0, 60)}`;
    } else if (typeof found.value !== "function") {
      reason = `not a function (${typeof found.value})`;
    } else {
      try {
        const verdict = test(found.value);
        if (verdict === true) {
          veena.log(`${kind}: ${candidate.label} matches (tested ${tested})`);
          return { label: candidate.label, fn: found.value, index: candidate.index };
        }
        reason = verdict;
      } catch (e) {
        reason = `threw ${String(e.message ?? e).slice(0, 60)}`;
      }
    }
    if (rejections.length < 5) rejections.push(`${candidate.label}: ${reason}`);
  }
  if (rejections.length > 0) {
    veena.log(`${kind}: none of ${tested} matched; e.g. ${rejections.join(" | ")}`);
  }
  return undefined;
}

/** Call code we do not control; engine limits become ordinary errors. */
function safely(fn, ...args) {
  const result = veena.attempt(fn, ...args);
  if (!result.ok) throw new Error(result.error);
  return result.value;
}

const SAMPLE_SIGNATURE = "AOq0QJ8wRAIgXmPlOPSBkkUs1bYFYlJCfe29xxLTnR1o6ZK7Vb1m1q4CIGR5Iu0ADBc0bMgDGNIq6GvdhZkTHvXlrrjZOq9Ki7Vf";
const SAMPLE_N = "a1Bc2De3Fg4Hi5Jk";

/** Shuffles a long string into a slightly shorter one of the same characters. */
function isSignatureFunction(fn) {
  const out = safely(fn, SAMPLE_SIGNATURE);
  if (typeof out !== "string") return `returned ${typeof out}`;
  if (out === SAMPLE_SIGNATURE) return "returned its input";
  if (out.length < 80 || out.length > SAMPLE_SIGNATURE.length) return `returned length ${out.length}`;
  const pool = {};
  for (const c of SAMPLE_SIGNATURE) pool[c] = (pool[c] ?? 0) + 1;
  for (const c of out) if (!pool[c]--) return `returned other characters: ${out.slice(0, 20)}`;
  return safely(fn, SAMPLE_SIGNATURE) === out ? true : "not repeatable";
}

const SAMPLE_N2 = "Zy9Xw8Vu7Ts6Rq5P";

/**
 * Turns a short code into a different short code of the same alphabet, the
 * same way each time, and different codes into different results (so a
 * function that maps everything to one word is rejected).
 */
function isNFunction(fn) {
  const out = safely(fn, SAMPLE_N);
  if (typeof out !== "string") return `returned ${typeof out}`;
  if (out === SAMPLE_N) return "returned its input";
  if (out.startsWith("enhanced_except")) return `reported failure: ${out.slice(0, 40)}`;
  if (!/^[A-Za-z0-9_-]{8,40}$/.test(out)) return `returned ${JSON.stringify(out.slice(0, 30))}`;
  const other = safely(fn, SAMPLE_N2);
  if (other === out) return "same result for different inputs";
  if (typeof other !== "string" || !/^[A-Za-z0-9_-]{8,40}$/.test(other)) return "second input failed";
  return safely(fn, SAMPLE_N) === out ? true : "not repeatable";
}

/** The n function reports failure by returning an error string. */
function wrapN(fn) {
  return (n) => {
    const out = safely(fn, n);
    if (typeof out !== "string" || out.startsWith("enhanced_except")) throw new Error("n function failed");
    return out;
  };
}

function wrapSignature(fn) {
  return (s) => {
    const out = safely(fn, s);
    if (typeof out !== "string") throw new Error("signature function failed");
    return out;
  };
}

// ---------------------------------------------------------------- Helpers

/** Development aid: log where a pattern occurs in the player code. */
function debugGrep(code, pattern, context) {
  const matches = [...code.matchAll(new RegExp(pattern, "g"))];
  veena.log(`debugGrep /${pattern}/: ${matches.length} matches`);
  for (const m of matches.slice(0, veena.config.debugLimit ?? 8)) {
    const around = code.slice(Math.max(0, m.index - context), m.index + m[0].length + context);
    veena.log(`@${m.index}: ${around.replace(/\s+/g, " ")}`);
  }
}

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
