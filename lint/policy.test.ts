import { spawn } from "node:child_process";
import { mkdirSync, rmSync, writeFileSync } from "node:fs";
import { createRequire } from "node:module";
import { dirname, join, resolve } from "node:path";
import { fileURLToPath } from "node:url";

import { afterAll, beforeAll, describe, it } from "vite-plus/test";

type Probe = { name: string; source: string; layer?: string; file?: string };
type RejectedProbe = Probe & { rule: string };

const require = createRequire(import.meta.url);
const repoRoot = resolve(dirname(fileURLToPath(import.meta.url)), "..");
const vp = join(dirname(require.resolve("vite-plus/package.json")), "bin/vp");
const sandbox = join(repoRoot, ".scratch", `lint-policy-${process.pid}`);
const layerProbeName = `.policy-probe-${process.pid}`;

const rejected: RejectedProbe[] = [
  {
    name: "unexplained empty catch blocks",
    rule: "no-empty",
    source: `export async function save(action: () => Promise<void>) {
try { await action(); } catch {}
}`,
  },
  {
    name: "checked inputs without a change handler or read-only intent",
    rule: "react(checked-requires-onchange-or-readonly)",
    source: `export function Probe() {
return <input type="checkbox" aria-label="Notifications" checked />;
}`,
  },
  {
    name: "a function that falls through without returning",
    rule: "typescript(TS7030)",
    source: `export function label(enabled: boolean) {
if (enabled) return "Ready";
}`,
  },
  {
    name: "a union member hidden by a default case",
    rule: "typescript(switch-exhaustiveness-check)",
    source: `export function label(status: "ready" | "failed") {
switch (status) {
  case "ready": return "Ready";
  default: return "Unknown";
}
}`,
  },
  {
    name: "buttons with implicit submit behavior",
    rule: "react(button-has-type)",
    source: `export function Probe() { return <button>Cancel</button>; }`,
  },
  {
    name: "coercive equality comparisons",
    rule: "eqeqeq",
    source: `export function isEqual(left: string | number, right: string | number) {
return left == right;
}`,
  },
  {
    name: "promise rejections without an Error",
    rule: "typescript(prefer-promise-reject-errors)",
    source: `export function save() { return Promise.reject("Failed to save"); }`,
  },
  {
    name: "missing effect dependencies",
    rule: "react-hooks-js(exhaustive-deps)",
    source: `import { useEffect } from "react";
export function Probe({ title }: { title: string }) {
useEffect(() => { document.title = title; }, []);
return null;
}`,
  },
  {
    name: "conditional hooks",
    rule: "react-hooks-js(rules-of-hooks)",
    source: `import { useEffect } from "react";
export function Probe({ enabled }: { enabled: boolean }) {
if (enabled) useEffect(() => { document.title = "Ready"; }, []);
return null;
}`,
  },
  {
    name: "derived state synchronized through an effect",
    rule: "react-hooks-js(set-state-in-effect)",
    source: `import { useEffect, useState } from "react";
export function Probe({ name }: { name: string }) {
const [label, setLabel] = useState("");
useEffect(() => { setLabel(name.toUpperCase()); }, [name]);
return <span>{label}</span>;
}`,
  },
  {
    name: "async effect callbacks",
    rule: "react-hooks-js(exhaustive-deps)",
    source: `import { useEffect } from "react";
export function Probe() {
useEffect(async () => { await Promise.resolve(); }, []);
return null;
}`,
  },
  {
    name: "components React Compiler cannot compile",
    rule: "react-hooks-js(todo)",
    source: `import { lazy } from "react";
export function Probe() {
const Panel = lazy(() => import("./panel"));
return <Panel />;
}`,
  },
  {
    name: "raw palette colors outside the theme",
    rule: "shadcn(no-raw-colors)",
    source: `export function Probe() { return <span className="bg-emerald-500" />; }`,
  },
  {
    name: "arbitrary appearance values",
    rule: "shadcn(no-arbitrary-values)",
    source: `export function Probe() { return <p className="text-[13px]">Ready</p>; }`,
  },
  {
    name: "padding on a kit component, even from the theme scale",
    rule: "shadcn(no-restyle)",
    source: `import { Button } from "@/components/ui/button";
export function Probe() { return <Button className="px-l">Save</Button>; }`,
  },
  {
    name: "restyling a kit component",
    rule: "shadcn(no-restyle)",
    source: `import { Button } from "@/components/ui/button";
export function Probe() { return <Button className="bg-brand">Save</Button>; }`,
  },
  {
    name: "Tailwind default scales the theme replaced",
    rule: "shadcn(no-unknown-classes)",
    source: `export function Probe() { return <p className="text-sm font-bold">Ready</p>; }`,
  },
  {
    name: "classes Tailwind cannot generate",
    rule: "shadcn(no-unknown-classes)",
    source: `export function Probe() { return <span className="rounded-huge" />; }`,
  },
  {
    name: "Promise methods called with extra arguments",
    rule: "typescript(TS2554)",
    source: `export const settled = Promise.resolve(1).catch(() => 0, () => 1);`,
  },
  {
    name: "classic Zod imports",
    rule: "eslint(no-restricted-imports)",
    source: `import { z } from "zod";
export const schema = z.object({ id: z.string() });`,
  },
  {
    name: "inline dependency suppressions",
    rule: "rodeo(no-disable-directives)",
    source: `/* oxlint-disable react-hooks-js/exhaustive-deps */
import { useEffect } from "react";
export function Probe({ title }: { title: string }) {
useEffect(() => { document.title = title; }, []);
return null;
}`,
  },
  {
    name: "throwing a value that is not an Error",
    rule: "typescript(only-throw-error)",
    source: `export function fail() { throw "Failed to save"; }`,
  },
  {
    name: "catch callbacks that type the error as any",
    rule: "typescript(use-unknown-in-catch-callback-variable)",
    source: `export const failure = Promise.reject(new Error("Failed")).catch((error) => error);`,
  },
  {
    name: "async functions that never await",
    rule: "typescript(require-await)",
    source: `export async function load() { return 1; }`,
  },
  {
    name: "a promise returned from try without await",
    rule: "typescript(return-await)",
    source: `export async function load(fetchItem: () => Promise<number>) {
try { return fetchItem(); } catch { return 0; }
}`,
  },
  {
    name: "browser dialogs",
    rule: "eslint(no-alert)",
    source: `export function notify() { alert("Saved"); }`,
  },
  {
    name: "comments without a why: reason",
    rule: "rodeo(no-comments)",
    source: `// Hero section\nexport const title = "Rodeo";`,
  },
  {
    name: "TODO notes, even behind why:",
    rule: "eslint(no-warning-comments)",
    source: `// why: TODO handle the error case later.\nexport const title = "Rodeo";`,
  },
  {
    name: "functions with too many branches",
    rule: "eslint(complexity)",
    source: `export function grade(score: number) {
${Array.from({ length: 21 }, (_, index) => `  if (score === ${index}) return "${index}";`).join("\n")}
return "none";
}`,
  },
  {
    name: "deeply nested blocks",
    rule: "eslint(max-depth)",
    source: `export function walk(flags: boolean[]) {
if (flags[0]) { if (flags[1]) { if (flags[2]) { if (flags[3]) { if (flags[4]) { return 1; } } } } }
return 0;
}`,
  },
  {
    name: "import cycles",
    rule: "import(no-cycle)",
    file: "cycle-probe.tsx",
    source: `import { partnerValue } from "./cycle-partner";
export const probeValue = 1;
export const total = partnerValue;`,
  },
  {
    name: "conditionals inside tests",
    rule: "vitest(no-conditional-in-test)",
    source: `import { expect, it } from "vite-plus/test";
it("parses", () => {
const value = Number("1");
if (value > 0) expect(value).toBe(1);
});`,
  },
  {
    name: "duplicate test titles",
    rule: "vitest(no-identical-title)",
    source: `import { expect, it } from "vite-plus/test";
it("parses", () => { expect(Number("1")).toBe(1); });
it("parses", () => { expect(Number("2")).toBe(2); });`,
  },
  {
    name: "raw import.meta.env reads",
    rule: "rodeo(no-import-meta-env)",
    source: `export const appUrl = import.meta.env.VITE_APP_URL;`,
  },
  {
    name: "src/lib importing a component",
    rule: "eslint(no-restricted-imports)",
    layer: "src/lib",
    source: `import { RouteErrorComponent } from "@/components/route-error";\nexport const mark = RouteErrorComponent;`,
  },
  {
    name: "src/hooks importing a component",
    rule: "eslint(no-restricted-imports)",
    layer: "src/hooks",
    source: `import { RouteErrorComponent } from "@/components/route-error";\nexport const mark = RouteErrorComponent;`,
  },
  {
    name: "the UI kit importing an app component",
    rule: "eslint(no-restricted-imports)",
    layer: "src/components/ui",
    source: `import { RouteErrorComponent } from "@/components/route-error";\nexport const mark = RouteErrorComponent;`,
  },
  {
    name: "importing a route module",
    rule: "eslint(no-restricted-imports)",
    layer: "src/components",
    source: `import { Route } from "@/routes/index";\nexport const route = Route;`,
  },
  {
    name: "Node built-ins in client code",
    rule: "eslint(no-restricted-imports)",
    layer: "src/lib",
    source: `import { readFileSync } from "node:fs";\nexport const read = readFileSync;`,
  },
  {
    name: "process.env in client code",
    rule: "eslint(no-restricted-globals)",
    layer: "src/lib",
    source: `export const apiKey = process.env.API_KEY;`,
  },
  {
    name: "oversized source files",
    rule: "eslint(max-lines)",
    layer: "src/lib",
    source: Array.from(
      { length: 301 },
      (_, index) => `export const value${index} = ${index};`,
    ).join("\n"),
  },
  {
    name: "unit tests in app code",
    rule: "eslint(no-restricted-imports)",
    layer: "src/lib",
    source: `import { expect, it } from "vite-plus/test";
it("adds", () => { expect(1 + 1).toBe(2); });`,
  },
];

const allowed: Probe[] = [
  {
    name: "explicit behavior and external synchronization with cleanup",
    source: `import { useEffect, useState } from "react";
import * as z from "zod/mini";
import { Button } from "@/components/ui/button";
export const itemSchema = z.object({ id: z.string(), tab: z.catch(z.enum(["a", "b"]), "a") });
export function Probe({ delay }: { delay: number }) {
  const [ticks, setTicks] = useState(0);
  useEffect(() => {
    const timer = window.setInterval(() => setTicks((value) => value + 1), delay);
    return () => window.clearInterval(timer);
  }, [delay]);
  return <button type="button">{ticks}</button>;
}
export function label(status: "ready" | "failed") {
  switch (status) {
    case "ready": return "Ready";
    case "failed": return "Failed";
  }
}
export function isEqual(left: string | number, right: string | number) {
  return left === right;
}
export function isAbsent(value: string | null | undefined) {
  return value == null;
}
export function save() {
  return Promise.reject(new Error("Failed to save"));
}
export async function attempt(action: () => Promise<void>) {
  try { await action(); } catch (error) { console.error(error); }
}
export async function removeCache(remove: () => Promise<void>) {
  try { await remove(); } catch {
    // why: Cache removal is best-effort, and a failure must not block signing out.
  }
}
export function ReadOnly() {
  return <input type="checkbox" aria-label="Notifications" checked readOnly />;
}
export function Editable() {
  const [checked, setChecked] = useState(false);
  return <input type="checkbox" aria-label="Notifications" checked={checked}
    onChange={(event) => setChecked(event.currentTarget.checked)} />;
}
export function Themed() {
  return (
    <>
      <Button className="mt-m w-full">Save</Button>
      <p className="text-s font-medium text-success">Ready</p>
    </>
  );
}`,
  },
  {
    name: "Node code in a server module",
    layer: "src/lib",
    file: "secrets.server.ts",
    source: `import { env } from "node:process";
export const apiKey = env.API_KEY;
export const region = process.env.REGION;`,
  },
  {
    name: "kit imports inside the kit",
    layer: "src/components/ui",
    source: `import { Download04Icon, Icon } from "@/components/icons";
import { Button } from "@/components/ui/button";
export function Probe() {
  return <Button type="button"><Icon icon={Download04Icon} aria-hidden="true" /> Export</Button>;
}`,
  },
];

// why: Path-scoped overrides match from the repository root, so a probe for src/lib must sit in
// src/lib. A dot directory keeps it out of TypeScript's include globs and Git.
function probePath({ name, layer, file }: Probe) {
  const directory = layer === undefined ? sandbox : join(repoRoot, layer, layerProbeName);
  return join(directory, file ?? `${name.replaceAll(/[^a-z0-9]+/gi, "-")}.tsx`);
}

const probes = [...rejected, ...allowed];
const probeDirectories = [
  ...new Set([sandbox, ...probes.map((probe) => dirname(probePath(probe)))]),
];

function isRecord(value: unknown): value is Record<string, unknown> {
  return typeof value === "object" && value !== null;
}

function codesByFile(json: string) {
  const parsed: unknown = JSON.parse(json);
  if (!isRecord(parsed) || !Array.isArray(parsed.diagnostics)) {
    throw new Error(`Unexpected lint output: ${json.slice(0, 500)}`);
  }
  const diagnostics: unknown[] = parsed.diagnostics;
  const codes = new Map<string, string[]>();
  for (const diagnostic of diagnostics) {
    if (
      !isRecord(diagnostic) ||
      typeof diagnostic.filename !== "string" ||
      typeof diagnostic.code !== "string"
    ) {
      throw new Error(`Unexpected diagnostic: ${JSON.stringify(diagnostic)}`);
    }
    const file = resolve(repoRoot, diagnostic.filename);
    codes.set(file, [...(codes.get(file) ?? []), diagnostic.code]);
  }
  return codes;
}

function lint(targets: string[]) {
  return new Promise<string>((resolvePromise, reject) => {
    const child = spawn(
      process.execPath,
      [vp, "lint", "--no-ignore", "--format", "json", ...targets],
      { cwd: repoRoot },
    );
    let stdout = "";
    child.stdout.setEncoding("utf8");
    child.stdout.on("data", (chunk: string) => {
      stdout += chunk;
    });
    child.on("error", reject);
    child.on("close", () => resolvePromise(stdout));
  });
}

let diagnostics = new Map<string, string[]>();

function codesFor(path: string) {
  return diagnostics.get(path) ?? [];
}

// why: One type-aware lint run over every probe takes several seconds, longer than the default
// hook timeout while CI builds in parallel.
beforeAll(async () => {
  for (const directory of probeDirectories) {
    mkdirSync(directory, { recursive: true });
    writeFileSync(
      join(directory, "tsconfig.json"),
      JSON.stringify({ extends: join(repoRoot, "tsconfig.app.json"), include: ["*.ts", "*.tsx"] }),
    );
  }
  writeFileSync(
    join(sandbox, "cycle-partner.tsx"),
    `import { probeValue } from "./cycle-probe";\nexport const partnerValue = probeValue + 1;\n`,
  );
  for (const probe of probes) writeFileSync(probePath(probe), probe.source);
  diagnostics = codesByFile(await lint(probes.map((probe) => probePath(probe))));
}, 60_000);

afterAll(() => {
  for (const directory of probeDirectories) rmSync(directory, { recursive: true, force: true });
});

describe("Lint policy", () => {
  it.for(rejected)("rejects $name", (probe, { expect }) => {
    expect(codesFor(probePath(probe))).toEqual(
      expect.arrayContaining([expect.stringContaining(probe.rule)]),
    );
  });

  it.for(allowed)("allows $name", (probe, { expect }) => {
    expect(codesFor(probePath(probe))).toEqual([]);
  });
});
