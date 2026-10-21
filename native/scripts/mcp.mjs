import fs from "node:fs";
import path from "node:path";

const [name, json = "{}"] = process.argv.slice(2);
if (!name) {
  console.error(`usage: node native/scripts/mcp.mjs <tool> ['{"arg": 1}']
Calls one of a running Filmlab's MCP tools and prints its answer. Images are saved to FILMLAB_MCP_IMAGES (default the
system temp folder) and their paths printed. FILMLAB_MCP_PORT picks the app (default 47428).`);
  process.exit(2);
}
const VERSION = "2026-07-28";
const port = process.env.FILMLAB_MCP_PORT || "47428";
const images = process.env.FILMLAB_MCP_IMAGES || path.join(process.env.TMPDIR || "/tmp", "filmlab-mcp");

const res = await fetch(`http://127.0.0.1:${port}/mcp`, {
  method: "POST",
  headers: {
    "Content-Type": "application/json",
    Accept: "application/json, text/event-stream",
    "MCP-Protocol-Version": VERSION,
    "Mcp-Method": "tools/call",
    "Mcp-Name": name,
  },
  body: JSON.stringify({
    jsonrpc: "2.0",
    id: 1,
    method: "tools/call",
    params: {
      name,
      arguments: JSON.parse(json),
      _meta: {
        "io.modelcontextprotocol/protocolVersion": VERSION,
        "io.modelcontextprotocol/clientCapabilities": {},
        "io.modelcontextprotocol/clientInfo": { name: "filmlab-mcp-cli", version: "1" },
      },
    },
  }),
  signal: AbortSignal.timeout(300000),
});
const text = await res.text();
const data = res.headers.get("content-type")?.includes("text/event-stream")
  ? text
      .split(/\r?\n/)
      .filter((l) => l.startsWith("data:"))
      .map((l) => l.slice(5).trim())
      .filter(Boolean)
      .at(-1)
  : text;
const reply = JSON.parse(data);
if (reply.error) {
  console.error(JSON.stringify(reply.error));
  process.exit(1);
}
const stamp = new Date().toISOString().replace(/[:.]/g, "-");
for (const [i, part] of reply.result.content.entries()) {
  if (part.type === "text") console.log(part.text);
  if (part.type === "image") {
    fs.mkdirSync(images, { recursive: true });
    const file = path.join(images, `${stamp}-${name}-${i}.${part.mimeType === "image/png" ? "png" : "jpg"}`);
    fs.writeFileSync(file, Buffer.from(part.data, "base64"));
    console.log(`image: ${file}`);
  }
}
process.exit(reply.result.isError ? 1 : 0);
