import { spawn, execFileSync } from "node:child_process";
import fs from "node:fs";
import net from "node:net";
import os from "node:os";
import path from "node:path";
import { fileURLToPath } from "node:url";

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

let imageCount = 0;
let wedged = null;
async function tool(name, args = {}, { allowError = false, label } = {}) {
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
  usage.push({ label: label || name, textBytes, imageBytes });
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
  const want = [
    "guide",
    "status",
    "open_folder",
    "list_photos",
    "set_flags",
    "open_photo",
    "get_edit",
    "set_controls",
    "set_film",
    "command",
    "catalog",
    "history",
    "profiles",
    "snapshots",
    "preview",
    "analyze",
    "view",
    "screenshot",
    "export",
  ];
  const have = toolList.map((t) => t.name);
  const missing = want.filter((n) => !have.includes(n));
  assert(!missing.length, `missing ${missing.join(",")}`);
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
  const neg = (find(st.json, "negatives") || find(st.json, "negative") || [])[0];
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
  const f = jpegSize(full.images[0]);
  const r = jpegSize(reg.images[0]);
  return `full ${f.w}x${f.h}, region ${r.w}x${r.h}`;
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
  await tool("set_controls", { values: { "light.exposure": -0.8 } });
  const feedbackFrom = async (i, cur, slope) => {
    if (i > 6) return { cur };
    const err = target - cur;
    console.log(`  iter ${i}: p50 ${cur.toFixed(4)} error ${err >= 0 ? "+" : ""}${err.toFixed(4)}`);
    if (Math.abs(err) <= 0.02) return { cur, converged: i - 1 };
    const ev = err / slope;
    const rel = (ev >= 0 ? "+" : "") + ev.toFixed(3);
    await tool(
      "set_controls",
      { values: { "light.exposure": rel } },
      { label: `set_controls(feedback ${i})` },
    );
    const next = await p50();
    const nextSlope =
      Math.abs(ev) > 0.01 && Math.abs(next - cur) > 0.002 ? (next - cur) / ev : slope;
    return feedbackFrom(i + 1, next, nextSlope);
  };
  const { cur, converged } = await feedbackFrom(1, await p50(), 0.2);
  if (converged !== undefined) return `converged in ${converged} step(s): p50 ${cur.toFixed(4)}`;
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
    assert(r.set.json?.photo === r.id, `set_controls answered for ${r.set.json?.photo}`);
    assert(r.film.json?.photo === r.id, `set_film answered for ${r.film.json?.photo}`);
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
