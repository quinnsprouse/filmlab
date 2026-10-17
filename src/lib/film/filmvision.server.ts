import { existsSync } from "node:fs";
import { readFile, readdir, stat } from "node:fs/promises";
import { join, relative, resolve, sep } from "node:path";

import * as z from "zod/mini";

const envSchema = z.object({
  FILMVISION_DIR: z.optional(z.string()),
});

const defaultFolder =
  "/Library/Application Support/Blackmagic Design/DaVinci Resolve/LUT/FilmVision Pro";
const kinds = ["Negative", "Print", "Utility"] as const;

function folder(): string {
  return envSchema.parse(process.env).FILMVISION_DIR ?? defaultFolder;
}

async function cubesIn(dir: string): Promise<string[]> {
  const entries = await readdir(dir, { withFileTypes: true });
  const nested = await Promise.all(
    entries
      .filter((entry) => !entry.name.startsWith("."))
      .map((entry) => {
        const path = join(dir, entry.name);
        if (entry.isDirectory()) {
          return cubesIn(path);
        }
        return Promise.resolve(entry.name.toLowerCase().endsWith(".cube") ? [path] : []);
      }),
  );
  return nested.flat();
}

async function index(): Promise<Response> {
  const root = folder();
  const present = kinds.map((kind) => join(root, kind)).filter((dir) => existsSync(dir));
  if (present.length === 0) {
    return new Response("FilmVision Pro isn't installed on this computer.", { status: 404 });
  }
  const files = (await Promise.all(present.map(cubesIn))).flat();
  const ids = files.map((file) => relative(root, file).replace(/\.cube$/i, "")).toSorted();
  return Response.json(ids, { headers: { "cache-control": "no-cache" } });
}

async function lut(path: string): Promise<Response> {
  const root = resolve(folder());
  const file = resolve(root, `${path}.cube`);
  if (!file.startsWith(root + sep) || !kinds.some((kind) => path.startsWith(`${kind}/`))) {
    return new Response("Not a FilmVision stock.", { status: 400 });
  }
  if (!existsSync(file) || !(await stat(file)).isFile()) {
    return new Response("That stock isn't installed.", { status: 404 });
  }
  return new Response(await readFile(file, "utf8"), {
    headers: { "content-type": "text/plain; charset=utf-8", "cache-control": "max-age=86400" },
  });
}

export function filmVisionResponse(path: string): Promise<Response> {
  return path === "index.json" ? index() : lut(decodeURIComponent(path).replace(/\.cube$/i, ""));
}
