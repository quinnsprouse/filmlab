import { spawnSync } from "node:child_process";
import { mkdtempSync, mkdirSync, rmSync, symlinkSync, writeFileSync } from "node:fs";
import { createRequire } from "node:module";
import { tmpdir } from "node:os";
import { dirname, join, resolve } from "node:path";
import { fileURLToPath } from "node:url";

import { afterAll, beforeAll, describe, expect, it } from "vite-plus/test";

import { rodeoRuleNames, type RodeoRuleName } from "./rodeo-rules";

const require = createRequire(import.meta.url);
const repoRoot = resolve(dirname(fileURLToPath(import.meta.url)), "..");
const pluginPath = join(repoRoot, "lint", "rules.js");
// why: oxlint is a dependency of vite-plus, not of this project, so it resolves from there instead
// of relying on npm hoisting a transitive package.
const requireFromVitePlus = createRequire(require.resolve("vite-plus/package.json"));
const oxlintBinary = resolve(
  dirname(requireFromVitePlus.resolve("oxlint/package.json")),
  "bin/oxlint",
);

// why: Each case starts oxlint, and startup can exceed the default timeout while CI builds in
// parallel.
const processTimeout = { timeout: 30_000 };

function isRuleName(value: string): value is RodeoRuleName {
  return (rodeoRuleNames as readonly string[]).includes(value);
}

type Diagnostic = { rule: RodeoRuleName; line: number; message: string };

function isRecord(value: unknown): value is Record<string, unknown> {
  return typeof value === "object" && value !== null;
}

function parseDiagnostics(json: string): Diagnostic[] {
  const parsed: unknown = JSON.parse(json);
  if (!isRecord(parsed) || !Array.isArray(parsed.diagnostics)) {
    throw new Error(`Unexpected oxlint output: ${json}`);
  }
  return parsed.diagnostics.map((entry: unknown) => {
    if (!isRecord(entry) || typeof entry.code !== "string" || typeof entry.message !== "string") {
      throw new Error(`Unexpected diagnostic: ${JSON.stringify(entry)}`);
    }
    const rule = entry.code.replace(/^rodeo\((.+)\)$/, "$1");
    if (!isRuleName(rule)) throw new Error(`Unexpected rule id: ${entry.code}`);
    const label: unknown = Array.isArray(entry.labels) ? entry.labels[0] : undefined;
    const span = isRecord(label) && isRecord(label.span) ? label.span : undefined;
    return { rule, line: typeof span?.line === "number" ? span.line : 0, message: entry.message };
  });
}

let sandbox: string;

beforeAll(() => {
  sandbox = mkdtempSync(join(tmpdir(), "rodeo-lint-rules-"));
  symlinkSync(join(repoRoot, "node_modules"), join(sandbox, "node_modules"), "dir");
  mkdirSync(join(sandbox, "src", "components", "ui"), { recursive: true });
  writeFileSync(
    join(sandbox, ".oxlintrc.json"),
    JSON.stringify({
      jsPlugins: [{ name: "rodeo", specifier: pluginPath }],
      categories: { correctness: "off" },
      rules: Object.fromEntries(rodeoRuleNames.map((name) => [`rodeo/${name}`, "error"])),
    }),
  );
});

afterAll(() => {
  rmSync(sandbox, { force: true, recursive: true });
});

function lint(source: string, file = "src/probe.tsx"): Diagnostic[] {
  const target = join(sandbox, file);
  mkdirSync(dirname(target), { recursive: true });
  writeFileSync(target, source);
  // why: Oxlint's config discovery is not reliable from inside a Vitest worker.
  const result = spawnSync(
    process.execPath,
    [oxlintBinary, "-c", ".oxlintrc.json", "--format", "json", file],
    {
      cwd: sandbox,
      encoding: "utf8",
    },
  );
  return parseDiagnostics(result.stdout);
}

function rulesHit(source: string, file?: string) {
  return lint(source, file).map((hit) => hit.rule);
}

describe("no-disable-directives", processTimeout, () => {
  it("reports every directive at the top of the file so no directive can hide itself", () => {
    const hits = lint(`/* oxlint-disable */
// oxlint-disable-next-line no-restricted-imports
const a = 1;
/* eslint-disable */
const b = 2; // oxlint-disable-line
export { a, b };`);
    expect(hits.map((hit) => hit.line)).toEqual([1, 1, 1, 1]);
    expect(hits.map((hit) => hit.message)).toEqual([
      expect.stringContaining("Line 1 has an inline lint suppression (oxlint-disable)"),
      expect.stringContaining("Line 2 has an inline lint suppression (oxlint-disable-next-line)"),
      expect.stringContaining("Line 4 has an inline lint suppression (eslint-disable)"),
      expect.stringContaining("Line 5 has an inline lint suppression (oxlint-disable-line)"),
    ]);
  });

  it("ignores ordinary comments that mention linting", () => {
    expect(
      rulesHit(`// why: Disabled features are listed in the docs.\nexport const a = 1;`),
    ).toEqual([]);
  });
});

describe("server-fn-requires-validator", processTimeout, () => {
  it("reports handlers that read data without a validator", () => {
    expect(
      rulesHit(`
import { createServerFn } from "@tanstack/react-start";
export const a = createServerFn({ method: "POST" }).handler(({ data }) => data);
export const b = createServerFn().handler((ctx) => ctx.data);`),
    ).toEqual(["server-fn-requires-validator", "server-fn-requires-validator"]);
  });

  it("allows validated handlers and handlers that ignore input", () => {
    expect(
      rulesHit(`
import { createServerFn } from "@tanstack/react-start";
export const a = createServerFn({ method: "POST" }).validator((d: unknown) => d).handler(({ data }) => data);
export const b = createServerFn({ method: "POST" }).inputValidator((d: unknown) => d).handler(({ data }) => data);
export const c = createServerFn({ method: "GET" }).handler(() => "ok");
export const d = createServerFn({ method: "GET" }).handler(({ context }) => context);`),
    ).toEqual([]);
  });
});

describe("no-state-from-props", processTimeout, () => {
  it("reports state seeded from a destructured or dotted prop", () => {
    expect(
      rulesHit(`
import { useState } from "react";
export function A({ value }: { value: number }) { const [v] = useState(value); return v; }
export function B(props: { count: number }) { const [c] = useState(props.count); return c; }`),
    ).toEqual(["no-state-from-props", "no-state-from-props"]);
  });

  it("allows default*/initial* props and unrelated initializers", () => {
    expect(
      rulesHit(`
import { useState } from "react";
export function A({ defaultValue, initialCount }: { defaultValue: number; initialCount: number }) {
  const [v] = useState(defaultValue);
  const [c] = useState(initialCount);
  const [n] = useState(0);
  return v + c + n;
}`),
    ).toEqual([]);
  });
});

describe("no-module-scope-browser-globals", processTimeout, () => {
  it("reports browser globals read at module scope", () => {
    expect(
      rulesHit(`
const saved = localStorage.getItem("x");
const width = window.innerWidth;
const ua = navigator.userAgent;
export { saved, width, ua };`),
    ).toEqual([
      "no-module-scope-browser-globals",
      "no-module-scope-browser-globals",
      "no-module-scope-browser-globals",
    ]);
  });

  it("allows typeof guards, function bodies, and shadowed names", () => {
    expect(
      rulesHit(`
const isBrowser = typeof window !== "undefined";
export function read() { return localStorage.getItem("x"); }
export const handler = () => document.title;
export function shadow(document: string) { return document; }
export const config = { window: 1, location: "/" };
export { isBrowser };`),
    ).toEqual([]);
  });
});

describe("no-window-navigation", processTimeout, () => {
  it("reports hard navigations", () => {
    expect(
      rulesHit(`
export function go() {
  window.location.href = "/a";
  location.href = "/b";
  window.location = "/c" as unknown as Location;
  window.location.assign("/d");
  location.replace("/e");
}`),
    ).toEqual(Array<RodeoRuleName>(5).fill("no-window-navigation"));
  });

  it("allows reading location", () => {
    expect(rulesHit(`export const path = () => window.location.pathname;`)).toEqual([]);
  });
});

function pluginRuleNames(pluginModule: unknown) {
  if (!isRecord(pluginModule) || !isRecord(pluginModule.default)) {
    throw new Error("lint/rules.js has no default export");
  }
  const { rules } = pluginModule.default;
  if (!isRecord(rules)) throw new Error("lint/rules.js exports no rules");
  return Object.keys(rules);
}

describe("registration", () => {
  it("registers every rule the plugin defines with Oxlint and React Doctor", async () => {
    const pluginModule: unknown = await import(pluginPath);

    expect(pluginRuleNames(pluginModule).toSorted()).toEqual(rodeoRuleNames.toSorted());
  });
});

describe("no-comments", processTimeout, () => {
  it("reports line, block, and JSX comments once per group", () => {
    const hits = lint(`// Section label
// that continues on the next line
export const a = 1; /* trailing note */
/** Describes the component. */
export function Probe() { return <div>{/* Hero */}</div>; }`);

    expect(hits.map((hit) => [hit.rule, hit.line])).toEqual([
      ["no-comments", 1],
      ["no-comments", 3],
      ["no-comments", 4],
      ["no-comments", 5],
    ]);
  });

  it("requires a reason of at least three words after why:", () => {
    const hits = lint(`// why: hack\nexport const a = 1;`);

    expect(hits.map((hit) => hit.message)).toEqual([expect.stringContaining("at least 3 words")]);
  });

  it("allows why: comments across continued lines, blocks, and JSX", () => {
    expect(
      rulesHit(`// why: Safari drops the first frame
// when the element starts hidden.
export const a = 1;
/* why: The spec requires a trailing slash here. */
export function Probe() { return <div>{/* why: Screen readers skip this decoration. */}</div>; }`),
    ).toEqual([]);
  });

  it("leaves TypeScript directives and lint directives to their own rules", () => {
    expect(
      rulesHit(`// @ts-expect-error The fixture is intentionally untyped.
export const a: number = "x";
// oxlint-disable-next-line no-console
console.log(a);`),
    ).toEqual(["no-disable-directives"]);
  });
});

describe("no-import-meta-env", processTimeout, () => {
  it("reports VITE_* reads and whole-object reads", () => {
    expect(
      rulesHit(`export const a = import.meta.env.VITE_APP_URL;
export const b = import.meta.env["VITE_KEY"];
export const { VITE_X } = import.meta.env;
export const c = import.meta["env"].VITE_KEY;
const { env } = import.meta;
export const d = env;`),
    ).toEqual(Array<RodeoRuleName>(5).fill("no-import-meta-env"));
  });

  it("allows Vite's built-in flags", () => {
    expect(
      rulesHit(`export const moduleUrl = import.meta.url;
export const flags = [
  import.meta.env.DEV,
  import.meta.env.PROD,
  import.meta.env.SSR,
  import.meta.env.MODE,
  import.meta.env.BASE_URL,
];`),
    ).toEqual([]);
  });
});
