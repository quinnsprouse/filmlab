import { spawn, spawnSync } from "node:child_process";
import { createHash } from "node:crypto";
import { existsSync, mkdirSync, readFileSync, statSync, writeFileSync } from "node:fs";
import { createRequire } from "node:module";
import { dirname, extname, join, resolve } from "node:path";
import { fileURLToPath } from "node:url";
import { stripVTControlCharacters } from "node:util";

const require = createRequire(import.meta.url);
const projectRoot =
  process.env.CLAUDE_PROJECT_DIR ?? resolve(dirname(fileURLToPath(import.meta.url)), "..", "..");
const stateFile = join(projectRoot, "node_modules", ".cache", "rodeo", "stop-check.json");
const checkableExtensions = new Set([
  ".cjs",
  ".css",
  ".cts",
  ".html",
  ".js",
  ".json",
  ".jsonc",
  ".jsx",
  ".md",
  ".mjs",
  ".mts",
  ".ts",
  ".tsx",
  ".yaml",
  ".yml",
]);
const outputLimit = 3_500;

function git(args) {
  const result = spawnSync("git", args, {
    cwd: projectRoot,
    encoding: "utf8",
    maxBuffer: 64 << 20,
  });
  if (result.status !== 0) throw new Error(`git ${args.join(" ")} failed: ${result.stderr}`);
  return result.stdout;
}

function lines(output) {
  return output.split("\n").filter(Boolean);
}

const gitEmptyTree = "4b825dc642cb6eb9a060e54bf8d69288fbee4904";

function baseRevision() {
  const head = spawnSync("git", ["rev-parse", "--verify", "--quiet", "HEAD"], { cwd: projectRoot });
  return head.status === 0 ? "HEAD" : gitEmptyTree;
}

function changedFiles(base) {
  const paths = [
    ...lines(git(["diff", "--name-only", base])),
    ...lines(git(["ls-files", "--others", "--exclude-standard"])),
  ];
  return [...new Set(paths)].filter((path) => existsSync(join(projectRoot, path)));
}

function fingerprint(base, files) {
  const hash = createHash("sha256");
  hash.update(git(["diff", base, "--binary"]));
  for (const path of lines(git(["ls-files", "--others", "--exclude-standard"])).toSorted()) {
    const stats = statSync(join(projectRoot, path), { throwIfNoEntry: false });
    hash.update(`${path}:${stats?.size ?? "deleted"}:${stats?.mtimeMs ?? 0}\n`);
  }
  hash.update(files.join("\n"));
  return hash.digest("hex");
}

function readState() {
  try {
    const value = JSON.parse(readFileSync(stateFile, "utf8"));
    return typeof value === "object" && value !== null ? value : {};
  } catch {
    return {};
  }
}

function writeState(state) {
  mkdirSync(dirname(stateFile), { recursive: true });
  writeFileSync(stateFile, `${JSON.stringify(state)}\n`);
}

function packageBinary(packageName, binaryName) {
  const packagePath = require.resolve(`${packageName}/package.json`);
  const packageJson = JSON.parse(readFileSync(packagePath, "utf8"));
  const binary =
    typeof packageJson.bin === "string" ? packageJson.bin : packageJson.bin?.[binaryName];
  if (!binary) throw new Error(`${packageName} does not expose ${binaryName}`);
  return resolve(dirname(packagePath), binary);
}

function run(label, binary, args) {
  return new Promise((resolvePromise) => {
    const child = spawn(process.execPath, [binary, ...args], {
      cwd: projectRoot,
      env: { ...process.env, CI: "1", FORCE_COLOR: "0", NO_COLOR: "1" },
    });
    let output = "";
    child.stdout.on("data", (chunk) => (output += chunk));
    child.stderr.on("data", (chunk) => (output += chunk));
    child.on("error", (error) => resolvePromise({ label, ok: false, output: String(error) }));
    child.on("close", (code) => resolvePromise({ label, ok: code === 0, output }));
  });
}

function clean(text) {
  return stripVTControlCharacters(text)
    .split("\n")
    .filter((line) => !line.startsWith("note: You are running `vp"))
    .join("\n");
}

function tail(text, limit) {
  const trimmed = clean(text).trim();
  return trimmed.length > limit ? `...${trimmed.slice(-limit)}` : trimmed;
}

function respond(output) {
  process.stdout.write(`${JSON.stringify(output)}\n`);
  process.exit(0);
}

try {
  const input = JSON.parse(readFileSync(0, "utf8"));
  if (input.hook_event_name !== "Stop") process.exit(0);
  if (!existsSync(join(projectRoot, ".git")) || !existsSync(join(projectRoot, "node_modules"))) {
    process.exit(0);
  }

  const base = baseRevision();
  const files = changedFiles(base);
  if (files.length === 0) process.exit(0);

  const state = readState();
  const current = fingerprint(base, files);
  if (state.passed === current) process.exit(0);

  const retriedWithoutChanges = input.stop_hook_active === true && state.failed === current;
  if (retriedWithoutChanges) {
    respond({
      systemMessage:
        "The stop check still fails on the current changes. Run `npm run check` to see the errors.",
    });
  }

  const vp = packageBinary("vite-plus", "vp");
  const checkable = files.filter((path) => checkableExtensions.has(extname(path).toLowerCase()));
  const results = await Promise.all([
    checkable.length > 0
      ? run("vp check (changed files)", vp, [
          "check",
          "--no-error-on-unmatched-pattern",
          ...checkable,
        ])
      : { label: "vp check", ok: true, output: "" },
    run("tsc -b", packageBinary("typescript", "tsc"), ["-b", "--pretty", "false"]),
    run("vp test run --changed", vp, [
      "test",
      "run",
      ...(base === "HEAD" ? ["--changed"] : []),
      "--passWithNoTests",
    ]),
  ]);

  const failures = results.filter((result) => !result.ok);
  if (failures.length === 0) {
    writeState({ ...state, passed: current });
    process.exit(0);
  }

  writeState({ ...state, failed: current });
  const perFailure = Math.floor(outputLimit / failures.length);
  respond({
    decision: "block",
    reason: [
      "The stop check found problems in the files you changed. Fix them before you finish.",
      ...failures.map((failure) => `${failure.label} failed:\n${tail(failure.output, perFailure)}`),
    ].join("\n\n"),
  });
} catch (error) {
  respond({
    systemMessage: `The stop check could not run: ${error instanceof Error ? error.message : String(error)}`,
  });
}
