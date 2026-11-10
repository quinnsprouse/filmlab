import { spawn, execFileSync } from "node:child_process";
import fs from "node:fs";
import net from "node:net";
import os from "node:os";
import path from "node:path";
import { fileURLToPath } from "node:url";
import zlib from "node:zlib";

const here = path.dirname(fileURLToPath(import.meta.url));
const binary = path.resolve(here, "../target/release/filmlab");
const fixtures = process.argv[2];
const VERSION = "2026-07-28";
const META = {
  "io.modelcontextprotocol/protocolVersion": VERSION,
  "io.modelcontextprotocol/clientCapabilities": {},
};

if (!fs.existsSync(binary)) {
  console.error(`missing ${binary}; run: cd native && cargo build --release`);
  process.exit(2);
}
if (!fixtures || !fs.existsSync(fixtures)) {
  console.error("usage: node native/scripts/mcp_e2e.mjs <photo folder>");
  process.exit(2);
}

const results = [];
class Skip extends Error {}
function report(status, name, detail = "") {
  results.push(status);
  console.log(`${status} ${name}${detail ? ": " + detail : ""}`);
}
async function check(name, fn) {
  try {
    const detail = await fn();
    report("PASS", name, detail ?? "");
  } catch (e) {
    if (e instanceof Skip) report("SKIP", name, e.message);
    else report("FAIL", name, e && e.message ? e.message : String(e));
  }
}
function assert(cond, msg) {
  if (!cond) throw new Error(msg);
}

function freePort(start) {
  return new Promise((resolve) => {
    const s = net.createServer();
    s.once("error", () => resolve(freePort(start + 1)));
    s.listen(start, "127.0.0.1", () => s.close(() => resolve(start)));
  });
}

const runDir = path.resolve(
  process.argv[3] || fs.mkdtempSync(path.join(os.tmpdir(), "filmlab-mcp-e2e-")),
);
const home = path.join(runDir, "home");
const photos = path.join(runDir, "photos");
const images = path.join(runDir, "images");
fs.mkdirSync(path.join(home, "Library/Application Support/Filmlab"), { recursive: true });
fs.mkdirSync(photos, { recursive: true });
fs.mkdirSync(images, { recursive: true });
for (const f of fs.readdirSync(fixtures)) {
  const src = path.join(fixtures, f);
  if (!fs.statSync(src).isFile()) continue;
  try {
    execFileSync("cp", ["-c", src, path.join(photos, f)], { stdio: "ignore" });
  } catch {
    fs.copyFileSync(src, path.join(photos, f));
  }
}
const PHOTOS_NEEDED = 9;
const found = fs.readdirSync(photos).filter((f) => /\.(dng|arw|cr2|cr3|nef|raf|orf|rw2|jpe?g|tiff?)$/i.test(f)).length;
if (found < PHOTOS_NEEDED) {
  console.error(`${fixtures} has ${found} photos; the multi-photo and paste checks need ${PHOTOS_NEEDED} different ones`);
  process.exit(2);
}

const port = await freePort(Number(process.env.FILMLAB_MCP_PORT) || 47520);
const mcpUrl = `http://127.0.0.1:${port}/mcp`;
const log = fs.openSync(path.join(runDir, "app.log"), "w");
const app = spawn(binary, [], {
  env: { ...process.env, HOME: home, FILMLAB_BACKGROUND: "1", FILMLAB_MCP_PORT: String(port) },
  stdio: ["ignore", log, log],
});
let appExited = false;
app.on("exit", () => (appExited = true));
function stopApp() {
  if (!appExited) app.kill("SIGTERM");
}
process.on("exit", stopApp);
for (const sig of ["SIGINT", "SIGTERM"]) process.on(sig, () => (stopApp(), process.exit(130)));

function parseJsonOrUndefined(text) {
  try {
    return JSON.parse(text);
  } catch {
    return undefined;
  }
}

async function parseBody(res) {
  const text = await res.text();
  const type = res.headers.get("content-type") || "";
  if (type.includes("text/event-stream")) {
    const msgs = [];
    for (const block of text.split(/\r?\n\r?\n/)) {
      const data = block
        .split(/\r?\n/)
        .filter((l) => l.startsWith("data:"))
        .map((l) => l.slice(5).trimStart())
        .join("\n");
      const msg = data ? parseJsonOrUndefined(data) : undefined;
      if (msg !== undefined) msgs.push(msg);
    }
    return { json: msgs.find((m) => m.id !== undefined) ?? msgs[msgs.length - 1], text, type };
  }
  const json = text ? parseJsonOrUndefined(text) : undefined;
  return { json, text, type };
}

let nextId = 1;
async function rpc(method, params = {}, opts = {}) {
  const headers = {
    "Content-Type": "application/json",
    Accept: "application/json, text/event-stream",
    "MCP-Protocol-Version": VERSION,
    "Mcp-Method": method,
    ...(method === "tools/call" ? { "Mcp-Name": params.name } : {}),
    ...opts.headers,
  };
  const body = {
    jsonrpc: "2.0",
    id: nextId++,
    method,
    params: opts.noMeta ? params : { ...params, _meta: META, ...opts.meta },
  };
  const res = await fetch(mcpUrl, {
    method: "POST",
    headers,
    body: JSON.stringify(body),
    signal: AbortSignal.timeout(opts.timeout ?? 120000),
  });
  const out = await parseBody(res);
  return { status: res.status, headers: res.headers, ...out };
}

const usage = [];
const tokens = (bytes) => Math.ceil(bytes / 4);

function jpegSize(buf) {
  assert(buf[0] === 0xff && buf[1] === 0xd8, "not a JPEG");
  let i = 2;
  while (i < buf.length) {
    if (buf[i] !== 0xff) {
      i++;
      continue;
    }
    const m = buf[i + 1];
    if (m === 0xd8 || m === 0x01 || (m >= 0xd0 && m <= 0xd7) || m === 0xff) {
      i += m === 0xff ? 1 : 2;
      continue;
    }
    const len = buf.readUInt16BE(i + 2);
    if (m >= 0xc0 && m <= 0xcf && m !== 0xc4 && m !== 0xc8 && m !== 0xcc)
      return { h: buf.readUInt16BE(i + 5), w: buf.readUInt16BE(i + 7) };
    i += 2 + len;
  }
  throw new Error("no SOF marker");
}

const clean = (o) => Object.fromEntries(Object.entries(o).filter(([, v]) => v !== undefined));
// The checks below were written against the operations by their own names; each is sent through the seven tools
// agents see, so every check exercises that surface.
function legacy(name, a) {
  switch (name) {
    case "status":
      return ["status", {}];
    case "open_folder":
      return ["photos", { action: "folder", args: a }];
    case "list_photos":
      return ["photos", { action: "list", args: a }];
    case "set_flags":
      return ["photos", { action: "flags", args: a }];
    case "open_photo":
      return ["photos", { action: "open", args: a }];
    case "export":
      return ["photos", { action: "export", args: a }];
    case "get_edit":
      return ["status", clean({ part: a.all ? "controls" : "state", photo: a.photo })];
    case "set_controls":
      return ["edit", clean({ photo: a.photo, values: a.values, label: a.label, preview: a.preview })];
    case "set_film": {
      const { photo, preview, ...film } = a;
      return ["edit", clean({ photo, preview, film })];
    }
    case "command":
      return ["edit", clean({ photo: a.photo, action: "command", args: clean({ name: a.name, params: a.params }), preview: a.preview })];
    case "fit": {
      const { photo, region, preview, ...goal } = a;
      return ["edit", clean({ photo, region, preview, fit: goal })];
    }
    case "paste": {
      const { to, ...rest } = a;
      return ["edit", { photo: to, paste: rest }];
    }
    case "catalog":
      if (a.kind === "controls") return ["guide", clean({ topic: "controls", filter: a.filter })];
      if (a.kind === "stocks") return ["guide", clean({ topic: "film", filter: a.filter })];
      return ["guide", { topic: "schema", filter: "command" + (a.filter ? "." + a.filter : "") }];
    case "history":
      if (a.action === "list") return ["status", clean({ part: "history", photo: a.photo })];
      return ["edit", clean({ photo: a.photo, action: a.action, args: a.step === undefined ? undefined : { step: a.step } })];
    case "snapshots":
      if (a.action === "list") return ["status", clean({ part: "snapshots", photo: a.photo })];
      return ["edit", clean({ photo: a.photo, action: "snapshot." + a.action, args: a.name === undefined ? undefined : { name: a.name } })];
    case "profiles": {
      const { action, ...rest } = a;
      return ["profiles", { action, args: rest }];
    }
    case "preview":
      return ["look", clean({ mode: a.try ? "trials" : "preview", photo: a.photo, edge: a.max_edge, region: a.region, to: a.before ? "before" : undefined, try: a.try, args: a.quality ? { quality: a.quality } : undefined })];
    case "analyze":
      return ["look", clean({ mode: "analyze", photo: a.photo, edge: a.max_edge, region: a.region, to: a.before ? "before" : undefined, scopes: a.scopes })];
    case "compare":
      return ["look", clean({ mode: "compare", photo: a.photo, from: a.from, to: a.to, region: a.region, edge: a.max_edge, image: a.image, args: a.values || a.detail ? clean({ values: a.values, detail: a.detail }) : undefined })];
    case "view":
      return ["view", { args: a }];
    case "screenshot":
      return ["look", clean({ mode: "screenshot", edge: a.max_edge })];
    default:
      return [name, a];
  }
}

function imageSize(buf) {
  if (buf.subarray(1, 4).toString() === "PNG") return { w: buf.readUInt32BE(16), h: buf.readUInt32BE(20), png: true };
  return { ...jpegSize(buf), png: false };
}

let imageCount = 0;
let wedged = null;
async function tool(oldName, oldArgs = {}, { allowError = false, label, raw = false } = {}) {
  const [name, args] = raw ? [oldName, oldArgs] : legacy(oldName, oldArgs);
  if (wedged) throw new Error(`skipped: app stopped answering at ${wedged}`);
  let r;
  try {
    r = await rpc("tools/call", { name, arguments: args }, { timeout: 200000 });
  } catch (e) {
    if (e.name === "TimeoutError" || e.name === "AbortError")
      wedged = `${name} ${JSON.stringify(args).slice(0, 100)}`;
    throw new Error(
      `${name} ${JSON.stringify(args).slice(0, 100)} timed out after 200 s (main thread hung?)`,
      { cause: e },
    );
  }
  assert(r.status === 200, `${name}: HTTP ${r.status} ${r.text.slice(0, 200)}`);
  assert(
    r.json && !r.json.error,
    `${name}: JSON-RPC error ${JSON.stringify(r.json && r.json.error)}`,
  );
  const res = r.json.result;
  assert(res && res.resultType === "complete", `${name}: resultType ${res && res.resultType}`);
  const parts = res.content || [];
  const text = parts
    .filter((p) => p.type === "text")
    .map((p) => p.text)
    .join("\n");
  const imgs = parts.filter((p) => p.type === "image").map((p) => Buffer.from(p.data, "base64"));
  const textBytes = Buffer.byteLength(text);
  const imageBytes = imgs.reduce((n, b) => n + b.length, 0);
  usage.push({ label: label || oldName, textBytes, imageBytes });
  imgs.forEach((b, i) =>
    fs.writeFileSync(
      path.join(images, `${String(++imageCount).padStart(2, "0")}-${name}${i ? "-" + i : ""}.jpg`),
      b,
    ),
  );
  if (res.isError && /isn't implemented yet/.test(text))
    throw new Skip(`${name} isn't implemented yet`);
  const json = parseJsonOrUndefined(text);
  if (res.isError && !allowError)
    throw new Error(`${name} ${JSON.stringify(args).slice(0, 120)} -> ${text.slice(0, 300)}`);
  return { text, json, images: imgs, isError: !!res.isError };
}

function find(obj, key) {
  if (obj && typeof obj === "object") {
    if (key in obj) return obj[key];
    for (const v of Object.values(obj)) {
      const f = find(v, key);
      if (f !== undefined) return f;
    }
  }
  return undefined;
}
function rowsOf(j) {
  if (j && Array.isArray(j.columns) && Array.isArray(j.rows))
    return j.rows.map((r) => Object.fromEntries(j.columns.map((c, i) => [c, r[i]])));
  const r = find(j, "photos") ?? find(j, "items") ?? (Array.isArray(j) ? j : []);
  return Array.isArray(r) ? r.map((x) => (typeof x === "string" ? { id: x } : x)) : [];
}
function strings(obj, out = []) {
  if (typeof obj === "string") out.push(obj);
  else if (obj && typeof obj === "object") Object.values(obj).forEach((v) => strings(v, out));
  return out;
}

const wait = (ms) => new Promise((r) => setTimeout(r, ms));
async function firstAccepted(variants, errs = []) {
  if (variants.length === 0) return { ok: undefined, errs };
  const [params, ...rest] = variants;
  const r = await tool("command", { name: "mask.adjust", params }, { allowError: true });
  if (!r.isError) return { ok: r, errs };
  return firstAccepted(rest, [...errs, r.text.slice(0, 80)]);
}
async function p50(args = {}) {
  const a = await tool("analyze", args);
  const v = a.json?.luma?.p50;
  assert(typeof v === "number", `analyze has no luma.p50: ${a.text.slice(0, 200)}`);
  return v;
}

async function serverIsUp() {
  try {
    const r = await rpc("server/discover", {}, { timeout: 2000 });
    return r.status === 200;
  } catch {
    return false;
  }
}

async function waitForServer(deadline) {
  if (appExited || Date.now() >= deadline) return false;
  if (await serverIsUp()) return true;
  await wait(500);
  return waitForServer(deadline);
}

{
  const up = await waitForServer(Date.now() + 60000);
  if (!up) {
    console.error(
      `FAIL startup: MCP server never answered on port ${port}; see ${path.join(runDir, "app.log")}`,
    );
    stopApp();
    process.exit(1);
  }
}
console.log(`# run dir ${runDir}, port ${port}, pid ${app.pid}`);

let toolList = [];
await check("protocol/discover", async () => {
  const r = await rpc("server/discover");
  assert(r.status === 200, `HTTP ${r.status}`);
  const res = r.json.result;
  assert(res.resultType === "complete", `resultType ${res.resultType}`);
  assert(
    res.supportedVersions?.includes(VERSION),
    `supportedVersions ${JSON.stringify(res.supportedVersions)}`,
  );
  assert(res.capabilities?.tools !== undefined, "capabilities.tools missing");
  return `versions ${res.supportedVersions.join(",")}`;
});

await check("protocol/tools-list", async () => {
  const a = await rpc("tools/list");
  const b = await rpc("tools/list");
  assert(a.status === 200, `HTTP ${a.status}`);
  const ra = a.json.result;
  assert(ra.resultType === "complete", `resultType ${ra.resultType}`);
  assert(typeof ra.ttlMs === "number", "ttlMs missing");
  assert(typeof ra.cacheScope === "string", "cacheScope missing");
  const names = ra.tools.map((t) => t.name);
  assert(
    JSON.stringify(names) === JSON.stringify(b.json.result.tools.map((t) => t.name)),
    "order differs between calls",
  );
  for (const t of ra.tools) {
    assert(t.name && t.description, `${t.name}: name/description`);
    assert(t.inputSchema?.type === "object", `${t.name}: inputSchema.type`);
    assert(t.annotations && typeof t.annotations === "object", `${t.name}: annotations missing`);
  }
  toolList = ra.tools;
  const bytes = Buffer.byteLength(JSON.stringify(ra.tools));
  usage.push({ label: "tools/list", textBytes: bytes, imageBytes: 0 });
  return `${names.length} tools, ${bytes} bytes (~${tokens(bytes)} tokens), ttlMs ${ra.ttlMs}, scope ${ra.cacheScope}`;
});

await check("protocol/legacy-initialize", async () => {
  const h = { "MCP-Protocol-Version": "2025-11-25" };
  const init = await fetch(mcpUrl, {
    method: "POST",
    headers: {
      "Content-Type": "application/json",
      Accept: "application/json, text/event-stream",
      ...h,
    },
    body: JSON.stringify({
      jsonrpc: "2.0",
      id: 1,
      method: "initialize",
      params: {
        protocolVersion: "2025-11-25",
        capabilities: {},
        clientInfo: { name: "e2e", version: "0" },
      },
    }),
  });
  const ib = await parseBody(init);
  assert(init.status === 200, `initialize HTTP ${init.status} ${ib.text.slice(0, 200)}`);
  assert(ib.json?.result?.protocolVersion, "initialize result lacks protocolVersion");
  const sid = init.headers.get("mcp-session-id");
  const sh = {
    "Content-Type": "application/json",
    Accept: "application/json, text/event-stream",
    ...h,
    ...(sid ? { "Mcp-Session-Id": sid } : {}),
  };
  const ni = await fetch(mcpUrl, {
    method: "POST",
    headers: sh,
    body: JSON.stringify({ jsonrpc: "2.0", method: "notifications/initialized" }),
  });
  await ni.text();
  assert(ni.status < 300, `notifications/initialized HTTP ${ni.status}`);
  const tl = await fetch(mcpUrl, {
    method: "POST",
    headers: sh,
    body: JSON.stringify({ jsonrpc: "2.0", id: 2, method: "tools/list" }),
  });
  const tb = await parseBody(tl);
  assert(tl.status === 200, `tools/list HTTP ${tl.status} ${tb.text.slice(0, 200)}`);
  assert(tb.json?.result?.tools?.length > 0, "no tools");
  return `negotiated ${ib.json.result.protocolVersion}, session ${sid ? "yes" : "none"}, ${tb.json.result.tools.length} tools`;
});

await check("protocol/version-header-mismatch-400", async () => {
  const r = await rpc("tools/list", {}, { headers: { "MCP-Protocol-Version": "2025-11-25" } });
  assert(r.status === 400, `expected 400, got ${r.status} ${r.text.slice(0, 160)}`);
  return "400";
});

await check("protocol/origin-403", async () => {
  const r = await rpc("tools/list", {}, { headers: { Origin: "http://evil.example" } });
  assert(r.status === 403, `expected 403, got ${r.status}`);
  return "403";
});

await check("protocol/get-405", async () => {
  const res = await fetch(mcpUrl, {
    method: "GET",
    headers: { Accept: "text/event-stream", "MCP-Protocol-Version": VERSION },
  });
  await res.text();
  assert(res.status === 405, `expected 405, got ${res.status}`);
  return "405";
});

await check("protocol/unknown-tool-error", async () => {
  const r = await rpc("tools/call", { name: "no_such_tool", arguments: {} });
  assert(
    r.json?.error?.code !== undefined,
    `expected JSON-RPC error, got HTTP ${r.status} ${r.text.slice(0, 200)}`,
  );
  return `code ${r.json.error.code}: ${r.json.error.message}`;
});

await check("protocol/tools-complete", () => {
  const want = ["guide", "status", "photos", "edit", "look", "view", "profiles"];
  const have = toolList.map((t) => t.name);
  assert(JSON.stringify(have.toSorted()) === JSON.stringify(want.toSorted()), `tools ${have.join(",")}`);
  return `${have.length} tools`;
});

let photoIds = [];
let first;

await check("session/guide-workflow", async () => {
  const g = await tool("guide", { topic: "workflow" });
  assert(g.text.length > 50, "guide empty");
  return `${g.text.length} chars`;
});
await check("session/status", async () => {
  const s = await tool("status");
  assert(s.json && s.json.mode, "no mode");
  return `mode ${s.json.mode}`;
});
await check("session/open_folder", async () => {
  const o = await tool("open_folder", { path: photos });
  const n = o.json?.photos;
  const expected = fs
    .readdirSync(photos)
    .filter((f) => /\.(dng|arw|cr3|nef|raf|jpe?g|tiff?|heic)$/i.test(f)).length;
  assert(n > 0, `photos ${n}`);
  return `${n} photos (${expected} files; DNG+JPG pairs may merge)`;
});
await check("session/list_photos", async () => {
  const all = await tool("list_photos", { limit: 50 });
  const rows = rowsOf(all.json);
  assert(rows.length, `no rows: ${all.text.slice(0, 200)}`);
  photoIds = rows.map((r) => r.id);
  first = photoIds[0];
  const page = await tool("list_photos", { offset: 1, limit: 2 });
  const prow = rowsOf(page.json);
  assert(
    prow.length === 2 && prow[0].id === photoIds[1],
    `paging: ${prow.map((r) => r.id)} vs ${photoIds[1]}`,
  );
  assert(page.json.total === rows.length, `total ${page.json.total} != ${rows.length}`);
  const unedited = rowsOf((await tool("list_photos", { filter: "unedited" })).json);
  assert(unedited.length === rows.length, `unedited ${unedited.length} vs ${rows.length}`);
  const picks = rowsOf((await tool("list_photos", { filter: "picks" })).json);
  return `${rows.length} rows, first ${first}, ${picks.length} picks`;
});
await check("session/set_flags", async () => {
  assert(first, "no photo");
  await tool("set_flags", { ids: [first], flag: "pick", rating: 4 });
  const row = rowsOf((await tool("list_photos", { filter: "picks" })).json).find(
    (r) => r.id === first,
  );
  assert(row, `picks filter lacks ${first}`);
  assert(row.rating === 4, `rating ${row.rating}`);
  return `${first} picked, 4 stars`;
});
await check("session/open_photo", async () => {
  assert(first, "no photo");
  const o = await tool("open_photo", { id: first });
  assert(o.json?.id === first, `id ${o.json?.id}`);
  return o.json.facts?.exposure ?? "";
});
await check("session/get_edit", async () => {
  const e = await tool("get_edit");
  assert(e.json !== undefined, "not JSON");
  const all = await tool("get_edit", { all: true });
  assert(all.text.length >= e.text.length, "all=true shorter than default");
  return `${e.text.length} bytes default, ${all.text.length} all`;
});
await check("session/catalog", async () => {
  const c = await tool("catalog", { kind: "controls", filter: "light" });
  assert(c.text.includes("light.exposure"), "controls filter light lacks light.exposure");
  const cmd = await tool("catalog", { kind: "commands", filter: "crop" });
  assert(cmd.text.includes("crop."), "commands filter crop lacks crop.*");
  assert(!/mask\./.test(cmd.text), "crop filter leaked mask commands");
  const st = await tool("catalog", { kind: "stocks" });
  assert(st.text.length > 50, "stocks empty");
  return `controls ${c.text.length}B, commands ${cmd.text.length}B, stocks ${st.text.length}B`;
});
await check("session/preview-size", async () => {
  const p = await tool("preview", { max_edge: 512 });
  assert(p.images.length === 1, `images ${p.images.length}`);
  const { w, h } = jpegSize(p.images[0]);
  assert(Math.max(w, h) <= 512, `${w}x${h} exceeds 512`);
  return `${w}x${h}, ${p.images[0].length} bytes`;
});
let baseP50;
await check("session/analyze", async () => {
  const a = await tool("analyze");
  assert(
    (a.json?.luma && a.json.hist16 !== undefined) || a.json?.["hist16%"],
    "missing luma/hist16",
  );
  baseP50 = a.json.luma.p50;
  const r = await tool("analyze", { region: [0, 0, 0.5, 0.5] });
  const before = await tool("analyze", { before: true });
  return `p50 ${baseP50}, top-left p50 ${r.json?.luma?.p50}, before p50 ${before.json?.luma?.p50}`;
});
await check("edit/set_controls-exposure", async () => {
  const before = await p50();
  await tool("set_controls", {
    values: { "light.exposure": 0.5 },
    label: "e2e exposure",
  });
  const after = await p50();
  assert(after > before, `p50 ${before} -> ${after} did not rise`);
  const e = await tool("get_edit");
  assert(/light\.exposure/.test(e.text), "get_edit lacks light.exposure");
  return `p50 ${before} -> ${after}`;
});
await check("edit/set_controls-relative", async () => {
  await tool("set_controls", { values: { "light.exposure": 0.5 } });
  await tool("set_controls", { values: { "light.exposure": "+0.2" } });
  const e = await tool("get_edit");
  const v = find(e.json, "light.exposure");
  assert(typeof v === "number" && Math.abs(v - 0.7) < 0.01, `light.exposure = ${v}, want 0.7`);
  return `exposure ${v}`;
});
await check("edit/set_controls-preview", async () => {
  const s = await tool("set_controls", { values: { "color.vibrance": 10 }, preview: 256 });
  assert(s.images.length === 1, "no preview image");
  const { w, h } = jpegSize(s.images[0]);
  assert(Math.max(w, h) <= 256, `${w}x${h}`);
  return `${w}x${h}`;
});
await check("edit/set_film", async () => {
  await tool("set_film", { gauge: 16, halation: { on: false } });
  const e = await tool("get_edit");
  assert(/16/.test(e.text), "get_edit film lacks gauge 16");
  return e.json?.film ? JSON.stringify(e.json.film).slice(0, 120) : "ok";
});
await check("edit/set_film-stocks", async () => {
  const st = await tool("catalog", { kind: "stocks" });
  const stocks = JSON.parse(st.text.slice(st.text.indexOf("\n{") + 1));
  const neg = stocks.negative?.[0];
  const negative = typeof neg === "string" ? neg : neg?.id || neg?.name;
  assert(negative, `no negative in catalog: ${st.text.slice(0, 200)}`);
  await tool("set_film", { negative });
  return `negative ${negative}`;
});
await check("edit/command-crop-aspect", async () => {
  await tool("command", { name: "crop.aspect", params: { aspect: "1x1" } });
  const p = await tool("preview", { max_edge: 512 });
  const { w, h } = jpegSize(p.images[0]);
  assert(Math.abs(w - h) <= 1, `${w}x${h} not square`);
  return `${w}x${h}`;
});
await check("edit/command-mask", async () => {
  const add = await tool("command", { name: "mask.add", params: { kind: "radial" } });
  const doc = await tool("catalog", { kind: "commands", filter: "mask.adjust" });
  const idv = find(add.json, "id") ?? find(add.json, "mask") ?? find(add.json, "index") ?? 0;
  const variants = [
    { id: idv, values: { exposure: 0.3 } },
    { values: { exposure: 0.3 } },
    { id: String(idv), values: { exposure: 0.3 } },
  ];
  const { ok, errs } = await firstAccepted(variants);
  assert(ok, `mask.adjust rejected all guesses (${errs[0]}); doc: ${doc.text.slice(0, 300)}`);
  const e = await tool("get_edit");
  assert(/mask/.test(e.text), "get_edit shows no masks");
  return `mask ${JSON.stringify(idv)} adjusted`;
});
await check("edit/history", async () => {
  const l = await tool("history", { action: "list" });
  const step = find(l.json, "step");
  const steps = find(l.json, "steps");
  assert(typeof step === "number", `list lacks step: ${l.text.slice(0, 200)}`);
  await tool("history", { action: "undo" });
  const s1 = (await tool("status")).json.history.step;
  assert(s1 === step - 1, `undo: step ${step} -> ${s1}`);
  await tool("history", { action: "redo" });
  const s2 = (await tool("status")).json.history.step;
  assert(s2 === step, `redo: step ${s2}, want ${step}`);
  await tool("history", { action: "go_to", step: 1 });
  const s3 = (await tool("status")).json.history.step;
  assert(s3 === 1, `go_to 1 -> ${s3}`);
  await tool("history", { action: "go_to", step });
  return `step ${step} of ${steps}`;
});
await check("edit/snapshots", async () => {
  await tool("set_controls", { values: { "light.exposure": 0.3 } });
  const a0 = await p50();
  await tool("snapshots", { action: "save", name: "e2e-a" });
  const list = await tool("snapshots", { action: "list" });
  assert(list.text.includes("e2e-a"), "list lacks e2e-a");
  await tool("set_controls", { values: { "light.exposure": 1.2 } });
  const a1 = await p50();
  assert(a1 > a0, "exposure change not measurable");
  await tool("snapshots", { action: "restore", name: "e2e-a" });
  const a2 = await p50();
  assert(Math.abs(a2 - a0) < 0.005, `restore p50 ${a2} != saved ${a0}`);
  await tool("snapshots", { action: "delete", name: "e2e-a" });
  const after = await tool("snapshots", { action: "list" });
  assert(!after.text.includes("e2e-a"), "snapshot still listed after delete");
  return `p50 ${a0} -> ${a1} -> restored ${a2}`;
});
await check("edit/profiles", async () => {
  const save = await tool("profiles", { action: "save", name: "e2e look" });
  const list = await tool("profiles", { action: "list" });
  assert(/e2e look/i.test(list.text), "list lacks saved profile");
  const id = find(save.json, "id") ?? "e2e look";
  await tool("profiles", { action: "apply", id, amount: 50 });
  await tool("profiles", { action: "delete", id });
  const after = await tool("profiles", { action: "list" });
  assert(!/e2e look/i.test(after.text), "profile still listed after delete");
  return `id ${JSON.stringify(id)}`;
});

const words = (r) => (r.json?.summary ?? "").split(", ");
async function steps() {
  return (await tool("status")).json.history;
}
await check("compare/identical", async () => {
  const r = await tool("compare", { from: "current", to: "current", image: "none" });
  assert(r.json?.identical === true, `same state differs: ${r.text.slice(0, 300)}`);
  assert(r.images.length === 0, "image none still sent an image");
  return r.json.summary;
});
await check("compare/dry-run", async () => {
  const before = await steps();
  const edit = (await tool("get_edit")).text;
  const r = await tool("compare", { values: { "light.exposure": "+1" } });
  assert(words(r).includes("brighter"), `exposure +1 reads "${r.json?.summary}"`);
  assert(r.json.now.p50 > r.json.was.p50, `p50 ${r.json.was.p50} -> ${r.json.now.p50}`);
  assert(r.images.length === 1, "no heatmap");
  const { w, h } = jpegSize(r.images[0]);
  assert(w === r.json.size[0] && h === r.json.size[1], `heatmap ${w}x${h}, size ${r.json.size}`);
  const after = await steps();
  assert(after.step === before.step && after.steps === before.steps, "a dry run added history");
  assert((await tool("get_edit")).text === edit, "a dry run changed the edit");
  return r.json.summary;
});
await check("compare/previous-step", async () => {
  await tool("set_controls", { values: { "color.saturation": -60 } });
  const r = await tool("compare", { image: "pair" });
  assert(words(r).includes("less saturated"), `saturation -60 reads "${r.json?.summary}"`);
  const { w } = jpegSize(r.images[0]);
  assert(w === r.json.size[0] * 2, `pair is ${w} wide for a ${r.json.size[0]} frame`);
  await tool("history", { action: "undo" });
  const back = await tool("compare", { from: "current", to: "current", image: "none" });
  assert(back.json.identical, "current differs from itself");
  return r.json.summary;
});
await check("compare/region", async () => {
  const r = await tool("compare", {
    values: { "effects.texture": 80 },
    region: [0.4, 0.4, 0.6, 0.6],
    max_edge: 256,
    image: "none",
  });
  assert(Math.max(...r.json.size) <= 256, `size ${r.json.size}`);
  assert(words(r).includes("more texture"), `texture +80 reads "${r.json?.summary}"`);
  return `${r.json.size.join("x")}: ${r.json.summary}`;
});
await check("compare/crop", async () => {
  await tool("command", { name: "crop.aspect", params: { aspect: "16x9" } });
  const r = await tool("compare", { image: "none" });
  assert(words(r).includes("geometry changed"), `crop reads "${r.json?.summary}"`);
  assert(r.json.frames, "no frames for differing crops");
  await tool("history", { action: "undo" });
  return `${r.json.summary}; ${JSON.stringify(r.json.frames)}`;
});
await check("compare/snapshot-and-before", async () => {
  await tool("snapshots", { action: "save", name: "e2e-compare" });
  await tool("set_controls", { values: { "light.exposure": "-0.7" } });
  const r = await tool("compare", { from: "e2e-compare", image: "none" });
  assert(words(r).includes("darker"), `exposure -0.7 from the snapshot reads "${r.json?.summary}"`);
  const b = await tool("compare", { from: "before", image: "none" });
  assert(b.json?.size, `from before: ${b.text.slice(0, 200)}`);
  await tool("history", { action: "undo" });
  await tool("snapshots", { action: "delete", name: "e2e-compare" });
  return `snapshot: ${r.json.summary}; before: ${b.json.summary}`;
});
await check("compare/errors", async () => {
  const bad = await tool("compare", { values: { "light.exposur": 1 } }, { allowError: true });
  assert(bad.isError && /light\.exposure/.test(bad.text), `unknown control: ${bad.text}`);
  const step = await tool("compare", { from: 9999 }, { allowError: true });
  assert(step.isError && /no step/.test(step.text), `missing step: ${step.text}`);
  const snap = await tool("compare", { from: "no such snapshot" }, { allowError: true });
  assert(snap.isError && /no snapshot/.test(snap.text), `missing snapshot: ${snap.text}`);
  return "unknown control, missing step and missing snapshot refused";
});

await check("view/zoom-100", async () => {
  await tool("view", { mode: "develop", tool: "edit", zoom: "100" });
  const s = (await tool("status")).json;
  assert(s.zoom === 100, `zoom ${s.zoom}`);
  assert(Array.isArray(s.view_region) && s.view_region.length === 4, "view_region missing");
  return `zoom ${s.zoom}, region ${JSON.stringify(s.view_region)}`;
});
await check("view/zoom-fit", async () => {
  const v100 = (await tool("status")).json.view_region;
  await tool("view", { zoom: "fit" });
  const s = (await tool("status")).json;
  assert(
    JSON.stringify(s.view_region) !== JSON.stringify(v100) || s.zoom !== 100,
    `fit changed nothing: zoom ${s.zoom}`,
  );
  return `zoom ${s.zoom}, region ${JSON.stringify(s.view_region)}`;
});
await check("view/tool", async () => {
  await tool("view", { tool: "crop" });
  const a = (await tool("status")).json.tool;
  await tool("view", { tool: "edit" });
  const b = (await tool("status")).json.tool;
  assert(a === "crop" && b === "edit", `tool ${a} / ${b}`);
  return "crop -> edit";
});
await check("view/compare", async () => {
  await tool("view", { compare: "before" });
  const a = (await tool("status")).json.compare;
  await tool("view", { compare: "after" });
  const b = (await tool("status")).json.compare;
  assert(a === "before" && b === "after", `compare ${a} / ${b}`);
  return "before -> after";
});
await check("view/preview-region", async () => {
  const full = await tool("preview", { max_edge: 512 });
  const reg = await tool("preview", { max_edge: 512, region: [0.25, 0.25, 0.75, 0.75] });
  assert(reg.images.length === 1, "no image");
  const f = imageSize(full.images[0]);
  const r = imageSize(reg.images[0]);
  assert(!f.png && r.png, `a whole frame should be JPEG and a region lossless: ${f.png} / ${r.png}`);
  assert(typeof reg.json?.scale === "number", `a region preview doesn't say its scale: ${reg.text}`);
  const one = await tool("preview", { max_edge: 512, region: [0.45, 0.45, 0.55, 0.55] });
  assert(one.json.scale <= 1.01, `a small region at 512 px isn't actual size: scale ${one.json.scale}`);
  return `full ${f.w}x${f.h}, region ${r.w}x${r.h} png at scale ${reg.json.scale}, small region at ${one.json.scale}`;
});
await check("view/screenshot", async () => {
  const s = await tool("screenshot", { max_edge: 1024 });
  assert(s.images.length === 1, "no image");
  const { w, h } = jpegSize(s.images[0]);
  assert(Math.max(w, h) <= 1024, `${w}x${h}`);
  return `${w}x${h}, ${s.images[0].length} bytes`;
});
await check("export", async () => {
  const e = await tool("export", { format: "jpeg", long_edge: 800 });
  const files = strings(e.json).filter((s) => /\.(jpe?g|png|tiff?)$/i.test(s));
  assert(files.length, `no file path in ${e.text.slice(0, 200)}`);
  const resolved = files.map((f) => (path.isAbsolute(f) ? f : path.join(photos, f)));
  const found = resolved.find((f) => fs.existsSync(f));
  assert(found, `file missing on disk: ${resolved.join(", ")}`);
  return `${found} (${fs.statSync(found).size} bytes)`;
});
await check("view/library-mode", async () => {
  await tool("view", { mode: "library" });
  const s = (await tool("status")).json;
  assert(s.mode === "library", `mode ${s.mode}`);
  await tool("view", { mode: "develop" });
  return "library";
});

await check("agent/feedback-loop", async () => {
  const target = 0.45;
  assert(first, "no photo");
  await tool("open_photo", { id: first });
  const start = await tool("edit", { values: { "light.exposure": -0.8 }, watch: ["p50"] }, { raw: true });
  const feedbackFrom = async (i, cur, slope) => {
    if (i > 6) return { cur };
    const err = target - cur;
    console.log(`  iter ${i}: p50 ${cur.toFixed(4)} error ${err >= 0 ? "+" : ""}${err.toFixed(4)}`);
    if (Math.abs(err) <= 0.02) return { cur, converged: i - 1 };
    const ev = err / slope;
    const rel = (ev >= 0 ? "+" : "") + ev.toFixed(3);
    const next = (
      await tool(
        "edit",
        { values: { "light.exposure": rel }, watch: ["p50"] },
        { label: `edit(feedback ${i})`, raw: true },
      )
    ).json.watch.p50;
    const nextSlope =
      Math.abs(ev) > 0.01 && Math.abs(next - cur) > 0.002 ? (next - cur) / ev : slope;
    return feedbackFrom(i + 1, next, nextSlope);
  };
  const { cur, converged } = await feedbackFrom(1, start.json.watch.p50, 0.2);
  if (converged !== undefined) return `converged in ${converged} step(s) on the edits' own numbers: p50 ${cur.toFixed(4)}`;
  console.log(`  final p50 ${cur.toFixed(4)}`);
  assert(
    Math.abs(target - cur) <= 0.02,
    `did not converge: p50 ${cur.toFixed(4)} after 6 iterations`,
  );
  return `p50 ${cur.toFixed(4)} after 6`;
});

const unescapeXml = (s) =>
  s
    .replaceAll("&quot;", '"')
    .replaceAll("&apos;", "'")
    .replaceAll("&lt;", "<")
    .replaceAll("&gt;", ">")
    .replaceAll("&amp;", "&");
function sidecar(id) {
  const file = path.join(photos, `${id.replace(/\.[^.]+$/, "")}.xmp`);
  assert(fs.existsSync(file), `no sidecar ${file}`);
  const text = fs.readFileSync(file, "utf8");
  const field = (name) => {
    const m = text.match(new RegExp(`<(\\w+):${name}>([\\s\\S]*?)</\\1:${name}>`));
    return m ? JSON.parse(unescapeXml(m[2])) : undefined;
  };
  return { settings: field("settings"), film: field("film") };
}
const exposureOf = (edit) => edit?.controls?.["light.exposure"] ?? 0;
const near = (a, b) => Math.abs(a - b) < 0.011;

async function agent(id, exposure, gauge) {
  const started = performance.now();
  const set = await tool(
    "set_controls",
    { photo: id, values: { "light.exposure": exposure } },
    { label: "set_controls(photo)" },
  );
  const film = await tool("set_film", { photo: id, gauge }, { label: "set_film(photo)" });
  const look = await tool("preview", { photo: id, max_edge: 512 }, { label: "preview(photo)" });
  return { id, ms: performance.now() - started, set, film, look };
}

const others = photoIds.filter((id) => id !== first);
const [photoA, photoB, photoC, photoD] = others;
let uiEdit;
let pair = [];

await check("concurrency/two-agents", async () => {
  assert(others.length >= 4, `need 4 photos besides the open one, have ${others.length}`);
  uiEdit = (await tool("get_edit")).text;
  const serialC = await agent(photoC, 0.3, 65);
  const serialD = await agent(photoD, -0.3, 120);
  const serial = serialC.ms + serialD.ms;
  const started = performance.now();
  pair = await Promise.all([agent(photoA, 0.6, 35), agent(photoB, -0.6, 16)]);
  const wall = performance.now() - started;
  for (const r of pair) {
    assert(r.set.json?.effect && r.film.json?.effect, `${r.id}: an edit answered without its effect`);
    assert(r.look.images.length === 1, `${r.id}: no preview image`);
  }
  assert(
    wall < serial,
    `concurrent ${wall.toFixed(0)} ms is not under the serial sum ${serial.toFixed(0)} ms`,
  );
  return `concurrent ${wall.toFixed(0)} ms (${pair.map((r) => r.ms.toFixed(0)).join(" + ")}) vs serial ${serialC.ms.toFixed(0)} + ${serialD.ms.toFixed(0)} = ${serial.toFixed(0)} ms`;
});

await check("concurrency/no-cross-talk", async () => {
  assert(pair.length === 2, "the two-agent run didn't finish");
  const [a, b] = await Promise.all([
    tool("get_edit", { photo: photoA }),
    tool("get_edit", { photo: photoB }),
  ]);
  assert(near(exposureOf(a.json), 0.6), `${photoA} exposure ${exposureOf(a.json)}, want 0.6`);
  assert(near(exposureOf(b.json), -0.6), `${photoB} exposure ${exposureOf(b.json)}, want -0.6`);
  assert(a.json?.film?.gauge === 35, `${photoA} gauge ${a.json?.film?.gauge}, want 35`);
  assert(b.json?.film?.gauge === 16, `${photoB} gauge ${b.json?.film?.gauge}, want 16`);
  const ui = (await tool("get_edit")).text;
  assert(ui === uiEdit, `the open photo's edit changed: ${uiEdit} -> ${ui}`);
  const s = (await tool("status")).json;
  assert(
    [photoA, photoB].every((id) => s.sessions?.includes(id)),
    `status sessions ${JSON.stringify(s.sessions)}`,
  );
  return `${photoA} ${exposureOf(a.json)} g${a.json.film.gauge}, ${photoB} ${exposureOf(b.json)} g${b.json.film.gauge}; sessions ${s.sessions.join(",")}`;
});

await check("concurrency/sidecars", async () => {
  await wait(300);
  const a = sidecar(photoA);
  const b = sidecar(photoB);
  assert(
    near(a.settings?.light?.exposure, 0.6),
    `${photoA} sidecar exposure ${a.settings?.light?.exposure}`,
  );
  assert(
    near(b.settings?.light?.exposure, -0.6),
    `${photoB} sidecar exposure ${b.settings?.light?.exposure}`,
  );
  assert(a.film?.grain?.amount !== b.film?.grain?.amount, "both sidecars carry the same grain");
  const rows = rowsOf((await tool("list_photos", { filter: "edited" })).json).map((r) => r.id);
  assert(rows.includes(photoA) && rows.includes(photoB), `edited filter ${rows.join(",")}`);
  return `${photoA} ${a.settings.light.exposure} grain ${a.film?.grain?.amount}, ${photoB} ${b.settings.light.exposure} grain ${b.film?.grain?.amount}`;
});

await check("concurrency/same-photo", async () => {
  const before = (await tool("history", { photo: photoA, action: "list" })).json;
  const bump = () =>
    tool(
      "set_controls",
      { photo: photoA, values: { "light.exposure": "+0.25" } },
      { label: "set_controls(photo)" },
    );
  const [x, y] = await Promise.all([bump(), bump()]);
  const values = [x, y].map((r) => r.json.set["light.exposure"]).toSorted((p, q) => p - q);
  assert(near(values[0], 0.85) && near(values[1], 1.1), `answers ${values.join(", ")}`);
  const after = (await tool("history", { photo: photoA, action: "list" })).json;
  assert(after.steps === before.steps + 2, `steps ${before.steps} -> ${after.steps}`);
  const tail = after.list.slice(-2).map(([i]) => i);
  assert(tail[0] !== tail[1], `steps ${tail.join(", ")}`);
  const edit = (await tool("get_edit", { photo: photoA })).json;
  assert(near(exposureOf(edit), 1.1), `exposure ${exposureOf(edit)}, want 1.1`);
  await wait(300);
  const disk = sidecar(photoA).settings?.light?.exposure;
  assert(near(disk, 1.1), `sidecar exposure ${disk}, want 1.1`);
  return `${values.join(" then ")}; steps ${before.steps} -> ${after.steps}; sidecar ${disk}`;
});

await check("concurrency/hand-over", async () => {
  const steps = (await tool("history", { photo: photoA, action: "list" })).json.steps;
  await tool("open_photo", { id: photoA });
  const edit = (await tool("get_edit")).json;
  assert(near(exposureOf(edit), 1.1), `the window shows exposure ${exposureOf(edit)}, want 1.1`);
  const s = (await tool("status")).json;
  assert(s.photo?.id === photoA, `open photo ${s.photo?.id}`);
  assert(!s.sessions?.includes(photoA), `${photoA} still has a session`);
  assert(
    s.history.steps === steps,
    `window history ${s.history.steps} steps, session had ${steps}`,
  );
  await tool("history", { action: "undo" });
  const undone = exposureOf((await tool("get_edit", { photo: photoA })).json);
  assert(near(undone, 0.85), `undo in the window -> exposure ${undone}, want 0.85`);
  return `window took ${steps} steps; undo -> ${undone}`;
});

const [photoE, photoF, photoG, photoH] = others.slice(4);
const lastLabel = async (id) => (await tool("history", { photo: id, action: "list" })).json.list.at(-1)[1];
const stepsOf = async (id) => (await tool("history", { photo: id, action: "list" })).json.step;

await check("agent/edit-answers", async () => {
  assert(photoE, `need 5 photos besides the open one, have ${others.length}`);
  const before = (await tool("analyze", { photo: photoE, max_edge: 512 })).json.luma.p50;
  const up = await tool("set_controls", { photo: photoE, values: { "light.exposure": 0.5 } });
  assert(/brighter/.test(up.json?.effect) && up.json.delta?.p50 > 0, `exposure +0.5 reads ${up.text.slice(0, 200)}`);
  const measured = (await tool("analyze", { photo: photoE, max_edge: 512 })).json.luma.p50;
  assert(Math.abs(measured - before - up.json.delta.p50) < 0.002, `delta p50 ${up.json.delta.p50}, analyze moved ${measured - before}`);
  const down = await tool("set_controls", { photo: photoE, values: { "light.exposure": 0 } });
  assert(/darker/.test(down.json?.effect) && down.json.delta?.p50 < 0, `exposure back to 0 reads ${down.text.slice(0, 200)}`);
  const mask = await tool("command", { photo: photoE, name: "mask.add", params: { kind: "radial" } });
  assert(mask.json?.effect === "unchanged at 512 px", `an unadjusted mask reads "${mask.json?.effect}"`);
  await tool("history", { photo: photoE, action: "undo" });
  return `${up.json.effect} / ${down.json.effect} / mask: ${mask.json.effect}`;
});

await check("agent/several-photos", async () => {
  const before = await Promise.all([photoF, photoG].map(stepsOf));
  const r = await tool("set_controls", { photo: [photoF, photoG], values: { "light.exposure": "+0.3" } });
  const results = r.json?.results ?? [];
  assert(
    results.length === 2 && results.every((x) => /brighter/.test(x.effect)),
    `answers ${r.text.slice(0, 300)}`,
  );
  assert(results.map((x) => x.photo).join() === [photoF, photoG].join(), `photos ${results.map((x) => x.photo)}`);
  const after = await Promise.all([photoF, photoG].map(stepsOf));
  assert(after.every((n, i) => n === before[i] + 1), `at steps ${before} -> ${after}`);
  const refused = await tool("set_controls", { photo: [photoF], values: { "light.exposure": 0 }, preview: 256 }, { allowError: true });
  assert(refused.isError && /preview/.test(refused.text), `preview with several photos: ${refused.text}`);
  await Promise.all([photoF, photoG].map((id) => tool("history", { photo: id, action: "undo" })));
  return results.map((x) => `${x.photo}: ${x.effect}`).join("; ");
});

await check("agent/fit", async () => {
  const before = await stepsOf(photoE);
  const r = await tool("fit", { photo: photoE, control: "light.exposure", measure: "p50", to: 0.45 });
  assert(r.json?.status === "reached" && Math.abs(r.json.p50 - 0.45) <= 0.004, `fit answered ${r.text.slice(0, 200)}`);
  const landed = (await tool("analyze", { photo: photoE, max_edge: 512 })).json.luma.p50;
  assert(Math.abs(landed - 0.45) <= 0.004, `analyze reads p50 ${landed} after the fit`);
  assert((await stepsOf(photoE)) === before + 1, "fit took more than one step");
  const like = await tool("fit", { photo: photoF, control: "light.exposure", measure: "p50", like: photoE });
  assert(like.json?.status === "reached" && Math.abs(like.json.p50 - landed) <= 0.006, `like: ${like.text.slice(0, 200)} vs ${landed}`);
  const warm = await tool("fit", { photo: photoE, control: "wb.temp", measure: "cast_b", to: 4 });
  const cast = (await tool("analyze", { photo: photoE, max_edge: 512 })).json.neutral_cast;
  assert(
    warm.json.status !== "reached" || Math.abs(cast.b - 4) <= 0.25,
    `fit says reached, analyze reads cast b ${cast.b}: ${warm.text.slice(0, 200)}`,
  );
  const bad = await tool("fit", { photo: photoE, control: "light.exposure", measure: "brightness", to: 1 }, { allowError: true });
  assert(bad.isError && /measures/.test(bad.text), `unknown measure: ${bad.text}`);
  const out = await tool("fit", { photo: photoE, control: "light.exposure", measure: "p50", to: 2 }, { allowError: true });
  assert(out.isError && /can't be reached/.test(out.text), `p50 2 was accepted: ${out.text}`);
  await tool("history", { photo: photoE, action: "undo" });
  return `exposure ${r.json.set["light.exposure"]} for p50 ${r.json.p50}; like ${like.json.set["light.exposure"]}; temp ${warm.json.set["wb.temp"]} ${warm.json.status} cast_b ${warm.json.cast_b} (analyze ${cast.b})`;
});

await check("edit/new-film-gauge", async () => {
  const before = (await tool("get_edit", { photo: photoE })).json?.film;
  assert(before === undefined || before === "off", `${photoE} already has film ${JSON.stringify(before)}`);
  await tool("set_film", { photo: photoE, negative: "Negative/Vision3 250D" });
  const film = (await tool("get_edit", { photo: photoE })).json?.film;
  assert(film?.gauge === 35, `new film ${JSON.stringify(film)}, want gauge 35`);
  await tool("history", { photo: photoE, action: "undo" });
  return `new film: gauge ${film.gauge}`;
});

await check("agent/paste", async () => {
  const profiles = (await tool("profiles", { action: "list" })).text;
  await tool("set_controls", { photo: photoE, values: { "color.saturation": 30, "effects.clarity": 20 } });
  await tool("set_film", { photo: photoE, gauge: 35 });
  const source = (await tool("get_edit", { photo: photoE })).json;
  const exposures = await Promise.all([photoG, photoH].map(async (id) => exposureOf((await tool("get_edit", { photo: id })).json)));
  const r = await tool("paste", { from: photoE, to: [photoG, photoH] });
  const results = r.json?.results ?? [];
  assert(results.length === 2 && results.every((x) => x.effect && x.effect !== "identical"), `answers ${r.text.slice(0, 300)}`);
  for (const [i, id] of [photoG, photoH].entries()) {
    const edit = (await tool("get_edit", { photo: id })).json;
    assert(edit.controls?.["color.saturation"] === 30 && edit.controls?.["effects.clarity"] === 20, `${id} controls ${JSON.stringify(edit.controls)}`);
    assert(edit.film?.gauge === 35 && edit.film?.negative === source.film?.negative, `${id} film ${JSON.stringify(edit.film)}`);
    assert(near(exposureOf(edit), exposures[i]), `${id} exposure moved: ${exposures[i]} -> ${exposureOf(edit)}`);
    assert((await lastLabel(id)) === `Paste from ${photoE}`, `${id} history ${await lastLabel(id)}`);
  }
  assert((await tool("profiles", { action: "list" })).text === profiles, "paste saved a profile");
  await tool("history", { photo: photoH, action: "undo" });
  const undone = (await tool("get_edit", { photo: photoH })).json;
  assert(undone.controls?.["color.saturation"] === undefined, `undo left ${JSON.stringify(undone.controls)}`);
  return results.map((x) => `${x.photo}: ${x.effect}`).join("; ");
});

await check("agent/paste-match", async () => {
  const r = await tool("paste", { from: photoE, to: photoH, match: ["exposure", "wb"] });
  const [one] = r.json?.results ?? [];
  assert(one && typeof r.json.from?.p50 === "number", `answer ${r.text.slice(0, 300)}`);
  const landed = (await tool("analyze", { photo: photoH, max_edge: 512 })).json;
  if (one.matched?.["light.exposure"] !== undefined)
    assert(Math.abs(landed.luma.p50 - r.json.from.p50) <= 0.004, `matched, but p50 ${landed.luma.p50} vs source ${r.json.from.p50}`);
  else assert(one.missed?.p50, `exposure neither matched nor missed: ${JSON.stringify(one)}`);
  if (one.matched?.["wb.temp"] !== undefined)
    assert(Math.abs(landed.neutral_cast.b - r.json.from.cast[1]) <= 0.3, `matched, but cast b ${landed.neutral_cast.b} vs ${r.json.from.cast[1]}`);
  assert((await lastLabel(photoH)) === `Paste from ${photoE}`, "not one paste step");
  const steps = await stepsOf(photoH);
  const again = await tool("paste", { from: photoE, to: photoH, match: ["exposure", "wb"], amount: 50 });
  assert(again.json.results?.[0]?.replaced, `a second paste from the same source didn't replace the first: ${again.text.slice(0, 200)}`);
  assert((await stepsOf(photoH)) === steps, `a re-paste stacked: at step ${steps} -> ${await stepsOf(photoH)}`);
  const none = await tool("paste", { from: photoE, to: photoH, amount: 0 }, { allowError: true });
  assert(none.isError, "a paste at amount 0 was accepted");
  return `matched ${JSON.stringify(one.matched)}, missed ${JSON.stringify(one.missed ?? {})}; p50 ${landed.luma.p50} vs ${r.json.from.p50}`;
});

await check("agent/contact-sheet", async () => {
  const ids = photoIds.slice(0, 9);
  const s = await tool("preview", { photo: ids });
  assert(s.images.length === 1, `images ${s.images.length}`);
  const { w, h } = jpegSize(s.images[0]);
  assert(Math.max(w, h) <= 1024, `${w}x${h}`);
  const rows = rowsOf(s.json);
  assert(rows.length === ids.length && rows.every((r, i) => r.photo === ids[i] && r["#"] === i + 1), `rows ${s.text.slice(0, 300)}`);
  const table = rowsOf((await tool("analyze", { photo: ids })).json);
  assert(table.length === ids.length && table.every((r) => r.p50 > 0), `table ${JSON.stringify(table).slice(0, 300)}`);
  const shown = (await tool("status")).json.photo?.id;
  const probe = ids.find((id) => id !== shown);
  const one = (await tool("analyze", { photo: probe, max_edge: 512 })).json.luma.p50;
  const row = table.find((r) => r.photo === probe);
  assert(Math.abs(row.p50 - one) < 0.002, `${probe}: table p50 ${row.p50}, analyze ${one}`);
  const region = await tool("preview", { photo: ids, region: [0, 0, 0.5, 0.5] }, { allowError: true });
  assert(region.isError, "a region on a contact sheet was accepted");
  return `${w}x${h} sheet of ${rows.length} (~${Math.ceil((w * h) / 750)} image tokens), table of ${table.length}`;
});

const call = (name, args, opts = {}) => tool(name, args, { ...opts, raw: true, label: opts.label ?? `${name}(${args.mode ?? args.action ?? args.topic ?? args.part ?? ""})` });

await check("surface/guide-schema", async () => {
  const list = await call("guide", { topic: "schema", filter: "photos.list" });
  assert(/filter\?/.test(list.text), `photos.list schema: ${list.text}`);
  const masks = await call("guide", { topic: "schema", filter: "command.mask" });
  assert(masks.text.includes("mask.add"), `command.mask schema lacks mask.add: ${masks.text.slice(0, 200)}`);
  const none = await call("guide", { topic: "schema", filter: "nothing.here" }, { allowError: true });
  assert(none.isError, "an unknown schema was answered");
  const metrics = await call("guide", { topic: "metrics" });
  return `photos.list ${list.text.length} chars, command.mask ${masks.text.length}, metrics ${metrics.text.length}`;
});

await check("surface/combined-edit", async () => {
  const before = (await call("status", { part: "history", photo: photoG })).json.step;
  const r = await call("edit", { photo: photoG, values: { "light.exposure": "+0.2" }, film: { gauge: 65 } });
  assert(r.json?.effect && r.json.set?.["light.exposure"] !== undefined, `answer ${r.text.slice(0, 200)}`);
  const h = (await call("status", { part: "history", photo: photoG })).json;
  assert(h.step === before + 1, `values and film took ${h.step - before} steps`);
  const label = h.list.at(-1)[1];
  assert(/Exposure/.test(label) && /Film/.test(label), `label ${label}`);
  const both = await call("edit", { photo: photoG, values: { "light.exposure": 0 }, fit: { control: "light.exposure", measure: "p50", to: 0.4 } }, { allowError: true });
  assert(both.isError, "values and fit together were accepted");
  return `${label}: ${r.json.effect}`;
});

await check("surface/if_state-and-watch", async () => {
  const state = (await call("status", { part: "state", photo: photoG })).json.state;
  assert(/^[0-9a-f]{16}$/.test(state ?? ""), `state ${state}`);
  const ok = await call("edit", { photo: photoG, values: { "color.vibrance": 10 }, if_state: state, watch: ["p50", "hi.b"] });
  assert(typeof ok.json.watch?.p50 === "number" && "hi.b" in ok.json.watch, `watch ${JSON.stringify(ok.json.watch)}`);
  const stale = await call("edit", { photo: photoG, values: { "color.vibrance": 20 }, if_state: state }, { allowError: true });
  assert(stale.isError && /conflict/.test(stale.text), `a stale if_state was accepted: ${stale.text}`);
  return `watch ${JSON.stringify(ok.json.watch)}`;
});

await check("surface/scopes", async () => {
  const r = await call("look", { mode: "analyze", photo: photoG, scopes: ["zones", "hues", "skin", "clip", "tail", "wave", "texture"] });
  assert(r.json?.zones && r.json.wave?.length === 8 && r.json.tail?.over?.length === 3, `scopes ${r.text.slice(0, 300)}`);
  const bad = await call("look", { mode: "analyze", photo: photoG, scopes: ["vectorscope"] }, { allowError: true });
  assert(bad.isError, "an unknown scope was accepted");
  return `${r.text.length} chars: ${Object.keys(r.json).join(", ")}`;
});

await check("surface/trials-commit", async () => {
  const steps = (await call("status", { part: "history", photo: photoG })).json;
  const t = await call("look", { mode: "trials", photo: photoG, try: [{ values: { "light.contrast": 20 } }, { film: { gauge: 16 } }] });
  assert(t.images.length === 1 && t.json?.tries?.length === 2 && t.json.state, `trials ${t.text.slice(0, 200)}`);
  const after = (await call("status", { part: "history", photo: photoG })).json;
  assert(after.steps === steps.steps, "trials saved something");
  const commit = await call("edit", { photo: photoG, values: { "light.contrast": 20 }, if_state: t.json.state });
  assert(commit.json?.effect, `commit ${commit.text.slice(0, 200)}`);
  return t.json.tries.map((x) => `${x["#"]}: ${x.effect}`).join("; ");
});

await check("guard/concurrent", async () => {
  const base = (await call("status", { part: "state", photo: photoF })).json.state;
  const [a, b] = await Promise.all([
    call("edit", { photo: photoF, fit: { control: "light.exposure", measure: "p50", to: 0.5 }, if_state: base }, { allowError: true }),
    call("edit", { photo: photoF, values: { "color.vibrance": 17 }, if_state: base }, { allowError: true }),
  ]);
  const landed = [a, b].filter((r) => !r.isError);
  assert(landed.length === 1, `${landed.length} changes guarded on one state both landed`);
  assert([a, b].some((r) => r.isError && /conflict/.test(r.text)), `the other didn't say conflict: ${a.text.slice(0, 120)} / ${b.text.slice(0, 120)}`);
  const list = await call("edit", { photo: [photoF], values: { "color.vibrance": 1 }, if_state: base }, { allowError: true });
  assert(list.isError, "if_state on a photo list was accepted");
  return `one landed, one conflict`;
});

await check("watch/concurrent", async () => {
  const [low, high] = await Promise.all([
    call("edit", { photo: photoG, values: { "light.exposure": 0.3 }, watch: ["p50"] }),
    call("edit", { photo: photoG, values: { "light.exposure": 1.5 }, watch: ["p50"] }),
  ]);
  assert(low.json.watch.p50 < high.json.watch.p50, `watches ${low.json.watch.p50} for +0.3 and ${high.json.watch.p50} for +1.5`);
  return `p50 ${low.json.watch.p50} at +0.3, ${high.json.watch.p50} at +1.5`;
});

await check("paste/disjoint-groups", async () => {
  await call("edit", { photo: photoE, values: { "light.contrast": 20, "color.vibrance": 30 } });
  await call("edit", { photo: photoH, paste: { from: photoE, groups: ["color"] } });
  await call("edit", { photo: photoH, paste: { from: photoE, groups: ["light"] } });
  const controls = (await call("status", { part: "state", photo: photoH })).json.controls ?? {};
  assert(controls["color.vibrance"] === 30 && controls["light.contrast"] === 20, `after colour then light: ${JSON.stringify(controls)}`);
  const again = await call("edit", { photo: photoH, paste: { from: photoE, groups: ["light"] } });
  assert(again.json.results[0].effect === "same settings", `the same paste again: ${again.text.slice(0, 200)}`);
  return "both groups kept; a repeat changes nothing";
});

await check("paste/bounded-match", async () => {
  await call("edit", { photo: photoE, values: { "light.exposure": 4 } });
  await call("edit", { photo: photoH, values: { "light.exposure": -4 } });
  const r = await call("edit", { photo: photoH, paste: { from: photoE, groups: ["color"], match: ["exposure"] } });
  const exposure = (await call("status", { part: "state", photo: photoH })).json.controls?.["light.exposure"];
  assert(exposure <= -1.99 && exposure >= -6.01, `a 2-stop match moved exposure from -4 to ${exposure}`);
  assert(r.json.results[0].missed?.p50?.bound, `the miss doesn't say its bound: ${r.text.slice(0, 300)}`);
  await call("edit", { photo: photoE, values: { "light.exposure": 0 } });
  return `exposure ${exposure}, ${JSON.stringify(r.json.results[0].missed)}`;
});

await check("fit/max_delta-and-like", async () => {
  const start = (await call("status", { part: "state", photo: photoF })).json.controls?.["light.exposure"] ?? 0;
  const r = await call("edit", { photo: photoF, fit: { control: "light.exposure", measure: "p50", to: 0.95, max_delta: 0.3 } });
  assert(Math.abs(r.json.set["light.exposure"] - start) <= 0.301 && r.json.status === "limit", `max_delta 0.3 from ${start}: ${r.text.slice(0, 200)}`);
  const bad = await call("edit", { photo: photoF, fit: { control: "light.exposure", measure: "p50", to: 0.5, max_delta: 0 } }, { allowError: true });
  assert(bad.isError, "max_delta 0 was accepted");
  const like = await call("edit", { photo: photoF, fit: { control: "wb.temp", measure: "mid.b", like: photoE } });
  assert(["reached", "near", "limit"].includes(like.json?.status), `like on a zone measure: ${like.text.slice(0, 200)}`);
  return `exposure ${start} -> ${r.json.set["light.exposure"]} (${r.json.status}); mid.b like ${photoE}: ${like.json.status}`;
});

await check("compare/region-lossless", async () => {
  const r = await call("look", { mode: "compare", photo: photoF, from: "before", region: [0.45, 0.45, 0.5, 0.5], edge: 512, image: "pair" });
  const size = imageSize(r.images[0]);
  assert(size.png && typeof r.json.scale === "number", `region compare: png ${size.png}, scale ${r.json.scale}`);
  const batch = await call("look", { mode: "analyze", photo: [photoF, photoG], scopes: ["zones"] }, { allowError: true });
  assert(batch.isError && /one photo/.test(batch.text), `scopes on a list: ${batch.text}`);
  return `${size.w}x${size.h} png at scale ${r.json.scale}`;
});

await check("request-id", async () => {
  const step = (await call("status", { part: "history", photo: photoF })).json.step;
  const ask = { photo: photoF, values: { "light.exposure": "+0.1" }, request_id: `e2e-${Date.now()}` };
  const first = await call("edit", ask);
  const resent = await call("edit", { ...ask, retry: true });
  assert(resent.text === first.text, "a resent request answered differently");
  const once = (await call("status", { part: "history", photo: photoF })).json.step;
  assert(once === step + 1, `the relative edit and its resend took ${once - step} steps`);
  const other = await call("edit", { ...ask, values: { "light.exposure": "+0.2" } }, { allowError: true });
  assert(other.isError && /different request/.test(other.text), `another request under the same id: ${other.text}`);
  const exposure = (await call("status", { part: "state", photo: photoF })).json.controls?.["light.exposure"];
  for (let i = 0; i < 260; i++) await call("edit", { photo: photoF, values: { "light.exposure": exposure }, request_id: `e2e-pad-${Date.now()}-${i}` }, { label: "edit(request padding)" });
  const padded = (await call("status", { part: "history", photo: photoF })).json.step;
  const late = await call("edit", { ...ask, retry: true }, { allowError: true });
  assert(late.isError && /outcome unknown/.test(late.text), `a resend after the id was forgotten: ${late.text.slice(0, 200)}`);
  const after = (await call("status", { part: "history", photo: photoF })).json.step;
  assert(after === padded, `the forgotten resend ran: at step ${padded} -> ${after}`);
  return "ran once; resend replayed; reuse refused; a forgotten resend left unknown and unrun";
});

await check("fit/rounds-inside-bound", async () => {
  await call("edit", { photo: photoG, values: { "light.exposure": 4 }, film: { negative: null } });
  const film = (await call("status", { part: "state", photo: photoG })).json.film;
  assert(film === "off", `film negative null left the film ${JSON.stringify(film)}`);
  const r = await call("edit", { photo: photoG, fit: { control: "light.exposure", measure: "white%", to: 100, max_delta: 0.2006 } });
  assert(r.json.set["light.exposure"] <= 4.2006 && r.json.status === "limit", `max_delta 0.2006 from 4: ${r.text.slice(0, 200)}`);
  await call("edit", { photo: photoG, values: { "light.exposure": 0 } });
  return `exposure ${r.json.set["light.exposure"]}, ${r.json.status}`;
});

await check("snapshot/guarded", async () => {
  const stale = (await call("status", { part: "state", photo: photoG })).json.state;
  await call("edit", { photo: photoG, values: { "color.vibrance": "+3" } });
  const save = await call("edit", { photo: photoG, action: "snapshot.save", args: { name: "e2e-stale" }, if_state: stale }, { allowError: true });
  assert(save.isError && /conflict/.test(save.text), `a snapshot saved under a stale state: ${save.text.slice(0, 160)}`);
  const watched = await call("edit", { photo: photoG, action: "snapshot.save", args: { name: "e2e-w" }, watch: ["p50"] }, { allowError: true });
  assert(watched.isError, "a watch on snapshot save was accepted");
  const listed = (await call("status", { part: "snapshots", photo: photoG })).text;
  assert(!/e2e-stale|e2e-w/.test(listed), `a refused snapshot was saved: ${listed}`);
  return "stale save refused; watch refused";
});

await check("compare/shifted-crops", async () => {
  await call("edit", { photo: photoG, action: "command", args: { name: "crop.set", params: { rect: [0, 0, 0.5, 0.5] } } });
  await call("edit", { photo: photoG, action: "snapshot.save", args: { name: "e2e-left" } });
  await call("edit", { photo: photoG, action: "command", args: { name: "crop.set", params: { rect: [0.5, 0.5, 1, 1] } } });
  const r = await call("look", { mode: "compare", photo: photoG, from: "e2e-left", region: [0.45, 0.45, 0.5, 0.5], image: "none" }, { allowError: true });
  assert(r.isError && /same place/.test(r.text), `a region compare across shifted crops: ${r.text.slice(0, 200)}`);
  const outside = await call("look", { mode: "preview", photo: photoG, region: [-0.2, 0.45, 0.05, 0.5] }, { allowError: true });
  assert(outside.isError, "a region outside 0-1 was accepted");
  await call("edit", { photo: photoG, action: "command", args: { name: "crop.reset" } });
  await call("edit", { photo: photoG, action: "snapshot.delete", args: { name: "e2e-left" } });
  return "refused";
});

await check("edit/film-null", async () => {
  await call("edit", { photo: photoG, film: { gauge: 35 } });
  await call("edit", { photo: photoG, film: { negative: null } });
  const off = (await call("status", { part: "state", photo: photoG })).json.film;
  assert(off === "off", `film-only negative null left ${JSON.stringify(off)}`);
  await call("edit", { photo: photoG, film: { negative: null } });
  const still = (await call("status", { part: "state", photo: photoG })).json.film;
  assert(still === "off", `negative null on an unfilmed photo turned film on: ${JSON.stringify(still)}`);
  return "off, and stays off";
});

await check("edit/nested-overrides", async () => {
  const tries = [
    ["fit photo", "edit", { photo: photoG, fit: { photo: photoF, control: "light.exposure", measure: "p50", to: 0.5 } }],
    ["film photo", "edit", { photo: photoG, film: { photo: photoF, gauge: 35 } }],
    ["paste to", "edit", { photo: photoG, paste: { from: photoE, groups: ["color"], to: photoF } }],
    ["profiles action", "profiles", { action: "list", args: { action: "save", name: "e2e-injected" } }],
  ];
  for (const [what, tool, args] of tries) {
    const r = await call(tool, args, { allowError: true });
    assert(r.isError && /goes outside/.test(r.text), `${what} wasn't refused: ${r.text.slice(0, 160)}`);
  }
  const profiles = (await call("profiles", { action: "list" })).text;
  assert(!/e2e-injected/.test(profiles), "a nested action saved a profile");
  const shot = await call("look", { mode: "screenshot", from: "before" }, { allowError: true });
  assert(shot.isError, "screenshot accepted from");
  return `${tries.length} refused`;
});

await check("fit/no-room-to-move", async () => {
  await call("edit", { photo: photoG, values: { "light.exposure": 4.0004 } });
  const step = (await call("status", { part: "history", photo: photoG })).json.step;
  const r = await call("edit", { photo: photoG, fit: { control: "light.exposure", measure: "white%", to: 100, max_delta: 0.0001 } });
  const after = (await call("status", { part: "history", photo: photoG })).json.step;
  assert(r.json.set["light.exposure"] === 4.0004 && r.json.status === "limit" && after === step, `a bound narrower than a step: ${r.text.slice(0, 200)}; at step ${step} -> ${after}`);
  await call("edit", { photo: photoG, values: { "light.exposure": 0 } });
  return `stayed at ${r.json.set["light.exposure"]} (${r.json.status})`;
});

await check("edit/unread-arguments", async () => {
  const state = (await call("status", { part: "state", photo: photoG })).json.state;
  await call("edit", { photo: photoG, values: { "color.vibrance": "+2" } });
  await call("edit", { photo: photoG, action: "snapshot.save", args: { name: "e2e-kept" } });
  const steps = (await call("status", { part: "history", photo: photoG })).json.step;
  const refused = [
    ["guard in command args", "edit", { photo: photoG, action: "command", args: { name: "develop.set", params: { values: { "light.exposure": 1 } }, if_state: state } }],
    ["guard in snapshot args", "edit", { photo: photoG, action: "snapshot.delete", args: { name: "e2e-kept", if_state: state } }],
    ["args without an action", "edit", { photo: photoG, values: { "light.exposure": 0.1 }, args: { name: "x" } }],
    ["photo inside a combined film", "edit", { photo: photoG, values: { "light.exposure": 0.1 }, film: { negative: null, photo: photoF } }],
    ["guard inside a combined film", "edit", { photo: photoG, values: { "light.exposure": 0.1 }, film: { gauge: 35, if_state: state } }],
    ["guard inside a trial film", "look", { mode: "trials", photo: photoG, try: [{ film: { gauge: 35, if_state: state } }] }],
    ["values on a preview", "look", { mode: "preview", photo: photoG, args: { values: { "light.exposure": 1 } } }],
    ["args list on analyze", "look", { mode: "analyze", photo: photoG, args: [] }],
    ["photo in compare args", "look", { mode: "compare", photo: photoG, args: { photo: photoF } }],
    ["preview on paste", "edit", { photo: photoG, paste: { from: photoE, groups: ["color"] }, preview: 64 }],
    ["label on paste", "edit", { photo: photoG, paste: { from: photoE, groups: ["color"] }, label: "mine" }],
  ];
  for (const [what, tool, args] of refused) {
    const r = await call(tool, args, { allowError: true });
    assert(r.isError, `${what} was accepted: ${r.text.slice(0, 160)}`);
  }
  const after = (await call("status", { part: "history", photo: photoG })).json.step;
  assert(after === steps, `a refused request changed the photo: at step ${steps} -> ${after}`);
  assert((await call("status", { part: "snapshots", photo: photoG })).text.includes("e2e-kept"), "a refused delete removed the snapshot");
  await call("edit", { photo: photoG, film: { gauge: 35 }, label: "e2e film label" });
  const label = (await call("status", { part: "history", photo: photoG })).json.list.at(-1)[1];
  assert(label === "e2e film label", `a film edit's label: ${label}`);
  return `${refused.length} refused, nothing changed; film label kept`;
});

await check("arguments/typed", async () => {
  const shown = (await call("status", {})).json.photo?.id;
  const before = (await call("status", { part: "state" })).json.state;
  const refused = [
    ["a number for photo", "edit", { photo: 42, film: { gauge: 16 } }],
    ["an object for photo", "edit", { photo: { id: shown }, values: { "light.exposure": 0.1 } }],
    ["a string for retry", "edit", { values: { "light.exposure": "+0.2" }, request_id: "e2e-never", retry: "true" }],
    ["retry without an id", "edit", { values: { "light.exposure": "+0.2" }, retry: true }],
    ["a string preview", "edit", { values: { "light.exposure": "+0.2" }, preview: "64" }],
    ["ids in command params", "edit", { action: "command", args: { name: "develop.set", params: { values: { "light.exposure": 1 }, ids: [shown] } } }],
    ["a guard in command params", "edit", { action: "command", args: { name: "develop.set", params: { values: { "light.exposure": 1 }, if_state: before } } }],
    ["at without zoom", "view", { args: { at: [0.8, 0.8] } }],
    ["an unknown zoom field", "view", { args: { zoom: { scale: 2, centre: 1 } } }],
    ["a folder in an export name", "photos", { action: "export", args: { name: "sub/name" } }],
    ["a filter on one text", "guide", { topic: "metrics", filter: "p50" }],
  ];
  for (const [what, tool, args] of refused) {
    const r = await call(tool, args, { allowError: true });
    assert(r.isError, `${what} was accepted: ${r.text.slice(0, 160)}`);
  }
  const after = (await call("status", { part: "state" })).json.state;
  assert(after === before, "a refused request changed the open photo");
  return `${refused.length} refused, nothing changed`;
});

await check("profiles/exact", async () => {
  const before = (await call("profiles", { action: "list" })).json.profiles?.length ?? 0;
  const a = (await call("profiles", { action: "save", args: { photo: photoE, name: "e2e-A", groups: ["color"] } })).json.saved.id;
  await call("profiles", { action: "save", args: { photo: photoE, name: a } });
  const unread = [
    { action: "list", args: { photo: photoE } },
    { action: "apply", args: { photo: photoG, id: a, groups: ["color"] } },
    { action: "delete", args: { id: a, name: "e2e-A" } },
    { action: "save", args: { name: "e2e-C", amount: 50 } },
    { action: "apply", args: { photo: photoG, id: a, amount: 300 } },
  ];
  for (const args of unread) {
    const r = await call("profiles", args, { allowError: true });
    assert(r.isError, `profiles ${JSON.stringify(args)} was accepted`);
  }
  await call("profiles", { action: "delete", args: { id: a } });
  const left = (await call("profiles", { action: "list" })).json.profiles ?? [];
  assert(!left.some((x) => x.id === a) && left.some((x) => x.name === a), `delete by id removed the wrong one: ${JSON.stringify(left)}`);
  await call("profiles", { action: "delete", args: { name: a } });
  assert(((await call("profiles", { action: "list" })).json.profiles?.length ?? 0) === before, "test profiles left behind");
  return "id and name resolve exactly; unread fields refused";
});

await check("profiles/film-amount", async () => {
  await call("edit", { photo: photoE, film: { gauge: 35 } });
  const id = (await call("profiles", { action: "save", args: { photo: photoE, name: "e2e-film", groups: ["film"] } })).json.saved.id;
  await call("edit", { photo: photoG, film: { negative: null } });
  const zero = await call("profiles", { action: "apply", args: { photo: photoG, id, amount: 0 } });
  const off = (await call("status", { part: "state", photo: photoG })).json.film;
  assert(off === "off", `a film profile at amount 0 left ${JSON.stringify(off)}; answer ${zero.text.slice(0, 120)}`);
  await call("profiles", { action: "apply", args: { photo: photoG, id, amount: 100 } });
  const on = (await call("status", { part: "state", photo: photoG })).json.film;
  assert(on?.gauge === 35, `a film profile at 100 left ${JSON.stringify(on)}`);
  await call("profiles", { action: "delete", args: { id } });
  await call("edit", { photo: photoE, film: { negative: null } });
  const offId = (await call("profiles", { action: "save", args: { photo: photoE, name: "e2e-no-film", groups: ["film"] } })).json.saved.id;
  await call("profiles", { action: "apply", args: { photo: photoG, id: offId, amount: 0 } });
  const kept = (await call("status", { part: "state", photo: photoG })).json.film;
  assert(kept?.gauge === 35, `a film-off profile at 0% changed the film: ${JSON.stringify(kept)}`);
  await call("profiles", { action: "apply", args: { photo: photoG, id: offId, amount: 100 } });
  const cleared = (await call("status", { part: "state", photo: photoG })).json.film;
  assert(cleared === "off", `a film-off profile at 100% left ${JSON.stringify(cleared)}`);
  await call("profiles", { action: "delete", args: { id: offId } });
  return `0%: ${off}; 100%: gauge ${on.gauge}; a film-off profile turns film off at 100% and leaves it at 0%`;
});

const curvy = others[1];
const curveOf = async (photo) => (await call("status", { part: "state", photo })).json.curve;
const historyOf = async (photo) => (await call("status", { part: "history", photo })).json;
const same = (a, b) => JSON.stringify(a) === JSON.stringify(b);
const clampLevel = (v) => Math.max(0, Math.min(255, Math.round(v)));

await check("curve/forms", async () => {
  assert(curvy, "no photo for the curve checks");
  await tool("open_photo", { id: curvy });
  await call("edit", { photo: curvy, action: "command", args: { name: "curve.reset", params: { channel: "all" } } });
  const s0 = (await historyOf(curvy)).step;
  const set = await call("edit", { photo: curvy, curve: { rgb: [[0, 0], [64, 51], [191, 204], [255, 255]] } });
  assert(set.json.curve?.tones?.["64"] === "64→51", `points answer ${set.text.slice(0, 300)}`);
  let h = await historyOf(curvy);
  assert(h.step === s0 + 1 && h.list.at(-1)[1] === "RGB curve", `points took ${h.step - s0} steps, label ${h.list.at(-1)[1]}`);
  assert(same((await curveOf(curvy))?.rgb, [[0, 0], [64, 51], [191, 204], [255, 255]]), "state lacks the points");
  const lift = await call("edit", { photo: curvy, curve: { rgb: { 64: "+8" } } });
  assert(lift.json.set?.["curve.rgb@64"] === 59, `relative answer ${lift.text.slice(0, 200)}`);
  h = await historyOf(curvy);
  assert(h.step === s0 + 2 && h.list.at(-1)[1] === "RGB curve 64 → 59", `relative label ${h.list.at(-1)[1]}`);
  const added = await call("edit", { photo: curvy, curve: { rgb: { 128: 300 } } });
  assert(added.json.clamped?.includes("curve.rgb@128 to 255"), `clamp ${added.text.slice(0, 200)}`);
  assert((await historyOf(curvy)).list.at(-1)[1] === "RGB curve 128 → 255", "the added point's label");
  assert(same((await curveOf(curvy))?.rgb, [[0, 0], [64, 59], [128, 255], [191, 204], [255, 255]]), `points after adding ${JSON.stringify(await curveOf(curvy))}`);
  await call("edit", { photo: curvy, action: "undo" });
  const still = await call("edit", { photo: curvy, curve: { red: null } });
  assert(!still.json.curve && (await historyOf(curvy)).step === s0 + 2, `a reset of a linear channel: ${still.text.slice(0, 200)}`);
  const preset = await call("edit", { photo: curvy, curve: { preset: "medium contrast" } });
  const state = await curveOf(curvy);
  assert(state?.preset === "Medium Contrast" && preset.json.curve, `preset state ${JSON.stringify(state)}`);
  assert((await historyOf(curvy)).list.at(-1)[1] === "Point curve Medium Contrast", "preset label");
  const both = await call("edit", { photo: curvy, values: { "light.exposure": "+0.1" }, curve: { blue: [[0, 10], [255, 245]] } });
  h = await historyOf(curvy);
  assert(h.step === s0 + 4 && /Exposure/.test(h.list.at(-1)[1]) && /Blue curve/.test(h.list.at(-1)[1]), `values and curve: ${h.list.at(-1)[1]}`);
  assert(both.json.curve?.neutral, `a blue curve moves neutrals: ${both.text.slice(0, 300)}`);
  await call("edit", { photo: curvy, action: "undo" });
  assert(same(await curveOf(curvy), state), "undo left the blue curve");
  const reset = await call("edit", { photo: curvy, curve: { rgb: null } });
  assert(!(await curveOf(curvy)) && reset.json.curve?.tones, `rgb null: ${JSON.stringify(await curveOf(curvy))}`);
  assert((await historyOf(curvy)).list.at(-1)[1] === "RGB curve reset", "reset label");
  const plain = await call("edit", { photo: curvy, values: { "light.exposure": "+0.05" } });
  assert(!plain.json.curve, `an exposure edit carried a curve block: ${plain.text.slice(0, 200)}`);
  const regional = await call("edit", { photo: curvy, values: { "curve.darks": 30 } });
  assert(regional.json.curve?.tones, `curve.darks has no curve block: ${regional.text.slice(0, 200)}`);
  await call("edit", { photo: curvy, action: "command", args: { name: "develop.curve", params: { channel: "green", points: [[0, 0], [0.5, 0.56], [1, 1]] } } });
  assert(same((await curveOf(curvy))?.green, [[0, 0], [128, 143], [255, 255]]), `a command's curve in state: ${JSON.stringify(await curveOf(curvy))}`);
  assert((await historyOf(curvy)).list.at(-1)[1] === "Green curve", "command label");
  await call("edit", { photo: curvy, action: "command", args: { name: "curve.reset", params: { channel: "all" } } });
  assert(!(await curveOf(curvy)) && (await historyOf(curvy)).list.at(-1)[1] === "Tone curve reset", "curve.reset all");
  const all = (await call("status", { part: "controls", photo: curvy })).json.curve;
  assert(Object.keys(all ?? {}).join() === "rgb,red,green,blue", `controls lists ${JSON.stringify(all)}`);
  return `labels and steps as the UI; preset ${state.preset}`;
});

await check("curve/refused", async () => {
  const before = (await call("status", { part: "state", photo: curvy })).json.state;
  const bad = [
    { luma: [[0, 0], [255, 255]] },
    { preset: "Medium Contrast", rgb: null },
    { rgb: [[64, 50], [64, 60]] },
    { rgb: { 64: 70, 66: 72 } },
    { regions: { darks: 10 } },
    { rgb: { 300: 20 } },
    { rgb: { 64: "up" } },
    { preset: "Faded" },
    [],
  ];
  for (const curve of bad) {
    const r = await call("edit", { photo: curvy, curve }, { allowError: true });
    assert(r.isError, `curve ${JSON.stringify(curve)} was accepted`);
  }
  for (const name of ["curve.savePreset", "curve.deletePreset"]) {
    const r = await call("edit", { photo: curvy, action: "command", args: { name, params: { name: "e2e-curve" } } }, { allowError: true });
    assert(r.isError && /profiles save/.test(r.text), `${name} wasn't pointed at profiles: ${r.text.slice(0, 160)}`);
  }
  const after = (await call("status", { part: "state", photo: curvy })).json.state;
  assert(after === before, "a refused curve changed the photo");
  return `${bad.length} curves and preset save/delete refused`;
});

await check("curve/notes", async () => {
  const bins = (await call("look", { mode: "analyze", photo: curvy, scopes: ["curve"] })).json.curve.hist.in;
  const peak = bins.slice(10, 54).reduce((best, b, i) => (b > bins[best] ? i + 10 : best), 10) * 4 + 2;
  const t = await call("look", {
    mode: "trials",
    photo: curvy,
    try: [
      { curve: { rgb: [[0, 0], [peak - 6, clampLevel(peak - 30)], [peak + 6, clampLevel(peak + 30)], [255, 255]] } },
      { curve: { rgb: [[0, 0], [peak - 20, peak - 20], [peak + 20, peak - 14], [255, 255]] } },
      { curve: { blue: [[0, 30], [128, 128], [255, 225]] } },
      { curve: { rgb: [[0, 0], [peak - 20, peak + 20], [peak + 20, peak - 20], [255, 255]] } },
      { curve: { preset: "Medium Contrast" } },
    ],
  });
  const notes = t.json.tries.map((row) => row.curve?.notes ?? []);
  const fired = (i, word) => notes[i].some((n) => n.startsWith(word));
  assert(fired(0, "banding"), `banding: ${JSON.stringify(notes[0])}`);
  assert(fired(1, "flattened"), `flattened: ${JSON.stringify(notes[1])}`);
  assert(fired(2, "crossover"), `crossover: ${JSON.stringify(notes[2])}`);
  assert(fired(3, "reversal"), `reversal: ${JSON.stringify(notes[3])}`);
  assert(notes[4].length === 0 && t.json.tries[4].curve?.tones, `Medium Contrast: ${JSON.stringify(t.json.tries[4].curve)}`);
  const steep = await call("edit", { photo: curvy, curve: { rgb: [[0, 0], [peak - 6, clampLevel(peak - 30)], [peak + 6, clampLevel(peak + 30)], [255, 255]] } });
  assert(steep.json.curve?.notes?.some((n) => n.startsWith("banding")), `an edit's notes: ${steep.text.slice(0, 300)}`);
  await call("edit", { photo: curvy, action: "undo" });
  return notes.slice(0, 4).map((n) => n[0]).join(" | ");
});

await check("curve/analyze", async () => {
  await call("edit", { photo: curvy, curve: { preset: "Medium Contrast" } });
  const region = [0.4, 0.4, 0.6, 0.6];
  const a = await call("look", { mode: "analyze", photo: curvy, scopes: ["curve", "glance"], region, image: "curve", from: "previous" });
  const c = a.json.curve;
  assert(c.map.rgb.length === 33 && c.map.rgb[8] === 51 && c.map.rgb[16] === 128 && c.map.rgb[32] === 255, `map ${JSON.stringify(c.map.rgb)}`);
  assert(same(c.points.rgb, [[0, 0], [64, 51], [128, 128], [191, 204], [255, 255]]), `points ${JSON.stringify(c.points)}`);
  const sum = (bins) => bins.reduce((n, b) => n + b, 0);
  assert(c.hist.in.length === 64 && Math.abs(sum(c.hist.in) - 1000) <= 40 && Math.abs(sum(c.hist.out) - 1000) <= 40, `hist ${JSON.stringify(c.hist)}`);
  const measured = a.json.glance.p50 * 255;
  assert(Math.abs(c.region.out.p50 - measured) <= 2, `region p50 out ${c.region.out.p50}, the render's ${measured.toFixed(1)}`);
  assert(c.region.in.p5 <= c.region.in.p50 && c.region.in.p50 <= c.region.in.p95, `region ${JSON.stringify(c.region)}`);
  const whole = (await call("look", { mode: "analyze", photo: curvy, scopes: ["glance"], edge: 512 })).json.glance.p50 * 255;
  let seen = 0;
  const median = c.hist.out.findIndex((b) => (seen += b) >= sum(c.hist.out) / 2) * 4 + 2;
  assert(Math.abs(median - whole) <= 5, `hist out median ${median}, the render's p50 ${whole.toFixed(1)}`);
  assert(a.images.length === 1 && imageSize(a.images[0]).w === 384 && /previous|step/.test(c.ghost ?? ""), `chart ${a.images.length} images, ghost ${c.ghost}`);
  const plain = await call("look", { mode: "analyze", photo: curvy, image: "curve" }, { allowError: true });
  const ghostless = await call("look", { mode: "analyze", photo: curvy, scopes: ["zones"], from: "before" }, { allowError: true });
  assert(plain.isError && ghostless.isError, "image curve without the scope, or from without the chart, was accepted");
  return `region in p50 ${c.region.in.p50} -> out ${c.region.out.p50} (render ${measured.toFixed(1)}), ${a.text.length} chars`;
});

await check("curve/trials-compare-fit", async () => {
  const t = await call("look", { mode: "trials", photo: curvy, try: [{ curve: { rgb: null } }, { curve: { preset: "Strong Contrast" } }] });
  assert(t.images.length === 1 && t.json.tries.length === 2 && t.json.tries[1].curve?.slope, `trials ${t.text.slice(0, 300)}`);
  const c = await call("look", { mode: "compare", photo: curvy, image: "none", args: { curve: { preset: "Strong Contrast" } } });
  assert(/more contrast/.test(c.json.summary) && c.json.curve?.tones, `compare ${c.text.slice(0, 300)}`);
  const s0 = (await historyOf(curvy)).step;
  const p50 = (await call("look", { mode: "analyze", photo: curvy, scopes: ["glance"], edge: 512 })).json.glance.p50;
  const at = Math.round((await call("look", { mode: "analyze", photo: curvy, scopes: ["curve"] })).json.curve.slope.at[1]);
  const fit = await call("edit", { photo: curvy, fit: { control: `curve.rgb@${at}`, measure: "p50", to: Math.round((p50 + 0.03) * 1000) / 1000 } });
  const h = await historyOf(curvy);
  const moved = fit.json.set?.[`curve.rgb@${at}`];
  assert(fit.json.status === "reached" && h.step === s0 + 1 && Number(/^RGB curve \d+ → (\d+)$/.exec(h.list.at(-1)[1])?.[1]) === Math.round(moved) && fit.json.curve?.tones, `fit ${fit.text.slice(0, 300)}; ${h.list.at(-1)[1]}`);
  return `fit set ${JSON.stringify(fit.json.set)}; ${t.json.tries.map((x) => x.effect).join(" / ")}`;
});

await check("curve/paste", async () => {
  const target = others[2];
  await call("edit", { photo: target, action: "command", args: { name: "curve.reset", params: { channel: "all" } } });
  const source = await curveOf(curvy);
  const r = await call("edit", { photo: target, paste: { from: curvy, groups: ["toneCurve"] } });
  assert(r.json.results?.[0]?.curve?.tones, `paste answer ${r.text.slice(0, 300)}`);
  const pastedCurve = await curveOf(target);
  assert(same(pastedCurve?.rgb, source?.rgb), `pasted ${JSON.stringify(pastedCurve)} from ${JSON.stringify(source)}`);
  return `carried ${JSON.stringify(pastedCurve.rgb)}`;
});

await check("curve/saved", async () => {
  const target = others[2];
  await call("edit", { photo: curvy, curve: { rgb: [[0, 18], [70, 74], [185, 196], [255, 244]] } });
  const kept = (await call("profiles", { action: "save", args: { photo: curvy, name: "E2E Faded", groups: ["toneCurve"] } })).json.saved;
  assert(kept?.id, "the curve wasn't saved as a profile");
  try {
    await call("edit", { photo: target, action: "command", args: { name: "curve.reset", params: { channel: "all" } } });
    const r = await call("edit", { photo: target, curve: { preset: "e2e faded" } });
    assert(!r.isError && r.json.curve?.black === "0→18", `applying the saved curve: ${r.text.slice(0, 300)}`);
    const applied = await curveOf(target);
    assert(applied?.preset === "E2E Faded" && same(applied?.rgb, [[0, 18], [70, 74], [185, 196], [255, 244]]), `state after the saved curve ${JSON.stringify(applied)}`);
    const steps = (await call("status", { part: "history", photo: target })).text;
    assert(/Tone curve E2E Faded/.test(steps), `history ${steps.slice(-300)}`);
    const missing = await call("edit", { photo: target, curve: { preset: "Nope" } }, { allowError: true });
    assert(missing.isError && /E2E Faded/.test(missing.text), `an unknown preset should list the saved curve: ${missing.text}`);
  } finally {
    await call("profiles", { action: "delete", args: { id: kept.id } });
  }
  return "saved as a profile, applied by name, named in state";
});

const EXPECT = [
  [{ "light.exposure": "+1" }, "brighter"],
  [{ "light.exposure": "-1" }, "darker"],
  [{ "light.contrast": 70 }, "more contrast"],
  [{ "light.contrast": -70 }, "less contrast"],
  [{ "wb.temp": "+1500" }, "warmer"],
  [{ "wb.temp": "-1500" }, "cooler"],
  [{ "wb.tint": "+40" }, "more magenta"],
  [{ "wb.tint": "-40" }, "greener"],
  [{ "color.saturation": 60 }, "more saturated"],
  [{ "color.saturation": -60 }, "less saturated"],
  [{ "color.vibrance": 60 }, "more saturated"],
  [{ "effects.texture": 80 }, "more texture"],
  [{ "effects.texture": -80 }, "less texture"],
  [{ "grain.amount": 80 }, "more texture"],
  [{ "vignette.amount": -80 }, "darker"],
];
await check("verify/directions", async () => {
  assert(first, "no photo");
  await tool("open_photo", { id: first });
  const wrong = [];
  for (const [values, want] of EXPECT) {
    const r = await tool("compare", { values, image: "none", max_edge: 512 }, { label: "compare(verify)" });
    if (!words(r).includes(want)) wrong.push(`${JSON.stringify(values)} reads "${r.json?.summary}", want ${want}`);
  }
  assert(!wrong.length, wrong.join("; "));
  return `${EXPECT.length} changes move the photo the way they should`;
});

const NEEDS = [
  [/^grain\.(size|roughness)$/, () => ({ values: { "grain.amount": 60 } })],
  [/^vignette\.(midpoint|roundness|feather|highlights)$/, () => ({ values: { "vignette.amount": -60 } })],
  [/^grading\.\w+\.hue$/, (id) => ({ values: { [id.replace(/hue$/, "sat")]: 60 } })],
  [
    /^grading\.(blending|balance)$/,
    () => ({
      values: {
        "grading.shadows.sat": 60,
        "grading.shadows.hue": 220,
        "grading.highlights.sat": 60,
        "grading.highlights.hue": 40,
      },
    }),
  ],
  [/^detail\.sharpenMasking$/, () => ({ values: { "detail.sharpenAmount": 100 } })],
  [/^detail\.nrDetail$/, () => ({ values: { "detail.nrLuminance": 60 } })],
  [/^detail\.nrColor(Detail|Smoothness)$/, () => ({ values: { "detail.nrColor": 60 } })],
  [/^optics\.vignettingMidpoint$/, () => ({ values: { "optics.vignetting": -60 } })],
  [/^optics\.defringePurpleHue/, () => ({ values: { "optics.defringePurple": 10 } })],
  [/^optics\.defringeGreenHue/, () => ({ values: { "optics.defringeGreen": 10 } })],
  [
    /^curve\.split/,
    () => ({
      values: { "curve.highlights": 60, "curve.lights": -60, "curve.darks": 60, "curve.shadows": -60 },
    }),
  ],
  [/^curve\.refineSaturation$/, () => ({ values: { "curve.lights": 80, "curve.darks": -80 } })],
  [/^bw\./, () => ({ command: { name: "develop.treatment", params: { bw: true } } })],
  [/^hdr\.sdr/, () => ({ command: { name: "develop.hdr", params: { enabled: true } } })],
];
const PHOTO_DEPENDENT = /^(mixer|bw)\.|^optics\.(profile|defringe)|^hdr\.maxEv$|^profile\.amount$/;
const GEOMETRY = /^geometry\.|^crop\.angle$|^optics\.distortion$/;
const FINE = /^(detail|enhance|grain)\.|^optics\.(defringe|ca)/;

await check("verify/every-control", async () => {
  assert(first, "no photo");
  await tool("open_photo", { id: first });
  const catalog = (await tool("catalog", { kind: "controls" })).json;
  const controls = Object.values(catalog)
    .filter(Array.isArray)
    .flat()
    .map((line) => {
      const [id, range] = line.split(" ");
      const [min, max] = range.split("..").map(Number);
      return { id, min, max };
    });
  const dead = [];
  const quiet = [];
  const geometry = [];
  for (const { id, min, max } of controls) {
    const need = NEEDS.find(([re]) => re.test(id))?.[1](id);
    const at = need && (await steps()).step;
    if (need?.values) await tool("set_controls", { values: need.values, label: "e2e prerequisite" });
    if (need?.command) await tool("command", need.command);
    const look = FINE.test(id) ? { region: [0.45, 0.45, 0.55, 0.55], max_edge: 640 } : { max_edge: 384 };
    const summaries = [];
    for (const v of [min, max, min + (max - min) / 3]) {
      const r = await tool(
        "compare",
        { values: { [id]: v }, image: "none", ...look },
        { label: "compare(verify)" },
      );
      summaries.push(r.json?.summary ?? r.text.slice(0, 80));
    }
    if (need) await tool("history", { action: "go_to", step: at });
    const changed = summaries.some((s) => !/^(identical|no visible change)$/.test(s));
    if (GEOMETRY.test(id) && !summaries.some((s) => s.startsWith("geometry changed")))
      geometry.push(`${id} (${summaries.join(" / ")})`);
    if (changed) continue;
    (PHOTO_DEPENDENT.test(id) ? quiet : dead).push(id);
  }
  if (quiet.length) console.log(`  no effect on this photo (depends on its colours, lens or HDR): ${quiet.join(", ")}`);
  assert(!geometry.length, `geometry controls not reported as geometry: ${geometry.join("; ")}`);
  assert(!dead.length, `no effect even with what they depend on set: ${dead.join(", ")}`);
  return `${controls.length - quiet.length} of ${controls.length} controls change the photo`;
});

const FILM_GRAIN = { color: 0.9, highlights: 1, size: 2.5, softness: 0.5, defocus: 0.7, amount: 0.3, structure: "cine" };
await check("verify/film-grain", async () => {
  assert(first, "no photo");
  await call("edit", { photo: first, film: { gauge: 35, grain: { amount: 0.6 } } });
  const dead = [];
  const seen = [];
  for (const [field, value] of Object.entries(FILM_GRAIN)) {
    await call("edit", { photo: first, film: { grain: { [field]: value } } });
    const r = await call("look", { mode: "compare", photo: first, region: [0, 0, 1, 1], edge: 1500, image: "none" });
    const summary = r.json?.summary ?? r.text.slice(0, 80);
    if (/^(identical|no visible change)$/.test(summary)) dead.push(`grain.${field}`);
    seen.push(`${field} ${summary}`);
  }
  assert(!dead.length, `film grain fields with no effect: ${dead.join(", ")}`);
  return seen.join("; ");
});

await check("edit/cine-grain-has-no-colour", async () => {
  assert(first, "no photo");
  await call("edit", { photo: first, film: { gauge: 35 } });
  const becoming = await call("edit", { photo: first, film: { grain: { structure: "cine", highlights: 0.5 } } }, { allowError: true });
  assert(becoming.isError && /belong to stills/.test(becoming.text), `highlights while turning cine: ${becoming.text}`);
  await call("edit", { photo: first, film: { grain: { structure: "cine", amount: 0.3 } } });
  const refused = await call("edit", { photo: first, film: { grain: { color: 0.5 } } }, { allowError: true });
  assert(refused.isError && /belong to stills/.test(refused.text), `color on cine: ${refused.text}`);
  const film = (await tool("get_edit", { photo: first })).json?.film;
  assert(film?.grain?.structure === "cine" && film.grain.color === undefined && film.grain.highlights === undefined, `cine summary ${JSON.stringify(film?.grain)}`);
  return `refused: ${refused.text.slice(0, 80)}`;
});

await check("curve/lightroom-sidecar", async () => {
  const folder = path.join(runDir, "sidecar");
  fs.mkdirSync(folder, { recursive: true });
  const source = fs.readdirSync(photos).find((f) => /\.dng$/i.test(f));
  assert(source, "no raw photo to copy");
  const raw = "SIDECAR.DNG";
  fs.copyFileSync(path.join(photos, source), path.join(folder, raw));
  const seq = (pairs) => `<rdf:Seq>${pairs.map(([x, y]) => `<rdf:li>${x}, ${y}</rdf:li>`).join("")}</rdf:Seq>`;
  const xmp = `<x:xmpmeta xmlns:x="adobe:ns:meta/"><rdf:RDF xmlns:rdf="http://www.w3.org/1999/02/22-rdf-syntax-ns#"><rdf:Description rdf:about="" xmlns:crs="http://ns.adobe.com/camera-raw-settings/1.0/" crs:Version="15.0" crs:ProcessVersion="11.0">
<crs:ToneCurvePV2012>${seq([[0, 12], [64, 58], [192, 200], [255, 245]])}</crs:ToneCurvePV2012>
<crs:ToneCurvePV2012Red>${seq([[0, 0], [255, 255]])}</crs:ToneCurvePV2012Red>
<crs:ToneCurvePV2012Green>${seq([[0, 0], [255, 255]])}</crs:ToneCurvePV2012Green>
<crs:ToneCurvePV2012Blue>${seq([[0, 10], [128, 126], [255, 240]])}</crs:ToneCurvePV2012Blue>
</rdf:Description></rdf:RDF></x:xmpmeta>`;
  fs.writeFileSync(path.join(folder, raw.replace(/\.dng$/i, ".xmp")), xmp);
  await tool("open_folder", { path: folder });
  const curve = (await call("status", { part: "state", photo: raw })).json.curve;
  assert(same(curve?.rgb, [[0, 12], [64, 58], [192, 200], [255, 245]]) && same(curve?.blue, [[0, 10], [128, 126], [255, 240]]) && !curve.red, `sidecar curve ${JSON.stringify(curve)}`);
  return JSON.stringify(curve);
});

await check("folders/same-name", async () => {
  const source = fs.readdirSync(photos).find((f) => /\.dng$/i.test(f));
  const [a, b] = ["twin-a", "twin-b"].map((name) => path.join(runDir, name));
  for (const folder of [a, b]) {
    fs.mkdirSync(folder, { recursive: true });
    for (const raw of ["SHOWN.DNG", "AGENT.DNG"]) fs.copyFileSync(path.join(photos, source), path.join(folder, raw));
  }
  await tool("open_folder", { path: a });
  await tool("open_photo", { id: "SHOWN.DNG" });
  await call("edit", { photo: "SHOWN.DNG", values: { "light.exposure": 1 } });
  await call("edit", { photo: "AGENT.DNG", values: { "light.contrast": 40 } });
  await tool("open_folder", { path: b });
  const shown = (await call("status", { part: "state", photo: "SHOWN.DNG" })).json;
  const agent = (await call("status", { part: "state", photo: "AGENT.DNG" })).json;
  assert(!shown.controls?.["light.exposure"], `the new folder's SHOWN.DNG read the old one's edit: ${JSON.stringify(shown.controls)}`);
  assert(!agent.controls?.["light.contrast"], `the new folder's AGENT.DNG read the old agent session: ${JSON.stringify(agent.controls)}`);
  const edited = await call("edit", { photo: "AGENT.DNG", values: { "light.whites": 10 } });
  assert(!edited.isError, `editing the new folder's AGENT.DNG failed: ${edited.text}`);
  const after = (await call("status", { part: "state", photo: "AGENT.DNG" })).json.controls ?? {};
  assert(!after["light.contrast"] && after["light.whites"] === 10, `the edit landed on the old session: ${JSON.stringify(after)}`);
  assert(/"light":\{"exposure":1(\.0+)?,/.test(fs.readFileSync(path.join(a, "SHOWN.xmp"), "utf8")), "the old folder's open photo wasn't saved before the switch");
  assert(!fs.existsSync(path.join(b, "SHOWN.xmp")), "the old photo's edit was written into the new folder");
  return "each folder's photo read and edited as its own";
});

function pngRows(buf) {
  assert(buf.subarray(1, 4).toString() === "PNG", "not a PNG");
  const idat = [];
  let header;
  for (let at = 8; at < buf.length; ) {
    const len = buf.readUInt32BE(at);
    const type = buf.subarray(at + 4, at + 8).toString();
    const data = buf.subarray(at + 8, at + 8 + len);
    if (type === "IHDR") header = { w: data.readUInt32BE(0), h: data.readUInt32BE(4), depth: data[8], color: data[9] };
    if (type === "IDAT") idat.push(data);
    at += 12 + len;
  }
  const { w, h, depth, color } = header;
  const channels = { 2: 3, 6: 4 }[color];
  assert(depth === 8 && channels, `PNG depth ${depth} colour type ${color}`);
  const stride = w * channels;
  const raw = zlib.inflateSync(Buffer.concat(idat));
  let prev = Buffer.alloc(stride);
  let flat = 0;
  let sum = 0;
  for (let y = 0; y < h; y++) {
    const filter = raw[y * (stride + 1)];
    const row = Buffer.from(raw.subarray(y * (stride + 1) + 1, (y + 1) * (stride + 1)));
    let lo = 255;
    let hi = 0;
    for (let i = 0; i < stride; i++) {
      const a = i >= channels ? row[i - channels] : 0;
      const b = prev[i];
      const c = i >= channels ? prev[i - channels] : 0;
      const p = a + b - c;
      const paeth = Math.abs(p - a) <= Math.abs(p - b) && Math.abs(p - a) <= Math.abs(p - c) ? a : Math.abs(p - b) <= Math.abs(p - c) ? b : c;
      const predicted = [0, a, b, (a + b) >> 1, paeth][filter];
      const v = (row[i] + predicted) & 255;
      row[i] = v;
      if (i % channels < 3) {
        lo = Math.min(lo, v);
        hi = Math.max(hi, v);
        sum += v;
      }
    }
    flat += hi <= 12 && hi - lo <= 12;
    prev = row;
  }
  return { w, h, flat, mean: sum / (w * h * 3) };
}

await check("export/full-size-right-after-heavy-edits", async () => {
  await tool("open_folder", { path: photos });
  let portrait;
  for (const id of photoIds) {
    const { w, h } = jpegSize((await call("look", { mode: "preview", photo: id, edge: 64 })).images[0]);
    if (h > w) {
      portrait = id;
      break;
    }
  }
  assert(portrait, "no portrait photo in the fixtures");
  await call("edit", { photo: portrait, film: { gauge: 35, grain: { structure: "stills", color: 0.3, highlights: 0.5 } } });
  const heavy = [
    { values: { "detail.sharpenAmount": 0, "effects.texture": -18, "effects.clarity": -8 }, film: { blur: 0.07, grain: { size: 1.6, amount: 0.17, softness: 0.12 }, halation: { on: true, amount: 0.9, radius: 6 } } },
    { values: { "detail.sharpenAmount": 0, "effects.texture": -30, "effects.clarity": -12 }, film: { blur: 0.1, grain: { size: 2.2, amount: 0.2, softness: 0.2 }, halation: { on: true, amount: 0.9, radius: 6 } } },
  ];
  const folder = path.join(runDir, "heavy");
  const means = [];
  const LIBRARY_LETS_GO_OF_THE_GPU_AFTER = 5000;
  const ROUNDS = 16;
  let took = 1200;
  let editing = 100;
  for (let k = 0; k < ROUNDS; k++) {
    const into = Math.round((took * (((7 * k) % ROUNDS) + 0.5)) / ROUNDS);
    await call("view", { args: { mode: "library" } });
    await wait(Math.max(0, LIBRARY_LETS_GO_OF_THE_GPU_AFTER - editing - into));
    const edited = Date.now();
    const answer = await call("edit", { photo: portrait, ...heavy[k % 2] });
    editing = Date.now() - edited;
    const started = Date.now();
    await call("photos", { action: "export", args: { ids: [portrait], format: "png", folder, name: `heavy${k}` } });
    took = Date.now() - started;
    const file = path.join(folder, fs.readdirSync(folder).find((f) => f.startsWith(`heavy${k}`)));
    const { w, h, flat, mean } = pngRows(fs.readFileSync(file));
    fs.rmSync(file);
    assert(h > w, `export ${k} is ${w}x${h}, not portrait`);
    assert(flat === 0, `export ${k} has ${flat} flat black rows of ${h}`);
    const moved = answer.json?.delta?.p50 ?? 0;
    assert(k < 2 || Math.abs(moved) < 0.3, `edit ${k} measured p50 moving ${moved}, as a black frame does`);
    means[k % 2] ??= mean;
    assert(Math.abs(mean - means[k % 2]) < 0.05, `export ${k} mean ${mean.toFixed(2)}, the same edit's first export ${means[k % 2].toFixed(2)}`);
  }
  return `${ROUNDS} full-size exports of ${portrait}, each straight after a heavy edit: no black rows, means ${means.map((m) => m.toFixed(2)).join(" / ")}`;
});

console.log("\n# token report (tokens ~ bytes/4)");
const byLabel = new Map();
for (const u of usage) {
  const e = byLabel.get(u.label) || { n: 0, text: 0, image: 0, max: 0 };
  e.n++;
  e.text += u.textBytes;
  e.image += u.imageBytes;
  e.max = Math.max(e.max, u.textBytes);
  byLabel.set(u.label, e);
}
let totalText = 0;
let totalImage = 0;
for (const [label, e] of [...byLabel].toSorted((a, b) => b[1].text - a[1].text)) {
  totalText += e.text;
  totalImage += e.image;
  const big = tokens(e.max) > 2000;
  console.log(
    `  ${label.padEnd(30)} calls ${String(e.n).padStart(2)}  text ${String(e.text).padStart(6)} B ~${String(tokens(e.text)).padStart(5)} tok  max ${String(tokens(e.max)).padStart(5)} tok  image ${e.image} B${big ? "  FLAG: text response over 2000 tokens" : ""}`,
  );
}
console.log(
  `  TOTAL text ${totalText} B ~${tokens(totalText)} tokens, images ${totalImage} B (${usage.length} responses)`,
);
const big = usage.filter((u) => tokens(u.textBytes) > 2000 && u.label !== "tools/list");
if (tokens(usage.find((u) => u.label === "tools/list")?.textBytes ?? 0) > 2000)
  console.log(
    "  NOTE tools/list is over 2000 tokens (sent on every turn); trim descriptions/schemas",
  );
if (big.length)
  report(
    "FAIL",
    "tokens/response-size",
    `over 2000 tokens: ${[...new Set(big.map((b) => b.label))].join(", ")}`,
  );
else report("PASS", "tokens/response-size", "no text response over 2000 tokens");

const count = (s) => results.filter((r) => r === s).length;
console.log(
  `\nSUMMARY pass ${count("PASS")}, fail ${count("FAIL")}, skip ${count("SKIP")}; run dir ${runDir}; images in ${images}`,
);
stopApp();
await wait(300);
process.exit(count("FAIL") ? 1 : 0);
