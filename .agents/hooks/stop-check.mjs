import { spawnSync } from "node:child_process";
import { createHash } from "node:crypto";
import { existsSync, readFileSync, statSync, writeFileSync } from "node:fs";
import { dirname, join, relative, resolve } from "node:path";
import { fileURLToPath } from "node:url";
import { stripVTControlCharacters } from "node:util";

const projectRoot =
  process.env.CLAUDE_PROJECT_DIR ?? resolve(dirname(fileURLToPath(import.meta.url)), "..", "..");
const dependents = { engine: ["engine", "native"], native: ["native"] };
const buildInput = /\.(?:rs|toml|lock|wgsl)$/;
const diagnostic = /^(\S+?):\d+:\d+: (warning|error)\b/;
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

function stateFile() {
  return resolve(projectRoot, git(["rev-parse", "--git-path", "filmlab-stop-check.json"]).trim());
}

function readState() {
  try {
    const value = JSON.parse(readFileSync(stateFile(), "utf8"));
    return typeof value === "object" && value !== null ? value : {};
  } catch {
    return {};
  }
}

function writeState(state) {
  writeFileSync(stateFile(), `${JSON.stringify(state)}\n`);
}

function cratesFor(files) {
  const crates = new Set();
  for (const path of files.filter((file) => buildInput.test(file))) {
    for (const crate of dependents[path.split("/")[0]] ?? []) crates.add(crate);
  }
  return [...crates];
}

function clippy(crate, changed) {
  const crateDir = join(projectRoot, crate);
  const result = spawnSync("cargo", ["clippy", "--all-targets", "--message-format", "short"], {
    cwd: crateDir,
    encoding: "utf8",
    maxBuffer: 64 << 20,
    env: { ...process.env, CARGO_TERM_COLOR: "never" },
  });
  if (result.error) return { label: `cargo clippy (${crate})`, output: String(result.error) };
  const output = stripVTControlCharacters(`${result.stdout}${result.stderr}`);
  const blocking = lines(output).filter((line) => {
    const match = diagnostic.exec(line);
    if (!match) return false;
    const path = relative(projectRoot, resolve(crateDir, match[1]));
    return match[2] === "error" || changed.has(path);
  });
  if (blocking.length > 0) return { label: `cargo clippy (${crate})`, output: blocking.join("\n") };
  if (result.status !== 0) return { label: `cargo clippy (${crate})`, output };
  return null;
}

function tail(text, limit) {
  const trimmed = text.trim();
  return trimmed.length > limit ? `...${trimmed.slice(-limit)}` : trimmed;
}

function respond(output) {
  process.stdout.write(`${JSON.stringify(output)}\n`);
  process.exit(0);
}

try {
  const input = JSON.parse(readFileSync(0, "utf8"));
  if (input.hook_event_name !== "Stop") process.exit(0);
  if (!existsSync(join(projectRoot, ".git"))) process.exit(0);

  const base = baseRevision();
  const files = changedFiles(base);
  const crates = cratesFor(files);
  if (crates.length === 0) process.exit(0);

  const state = readState();
  const current = fingerprint(base, files);
  if (state.passed === current) process.exit(0);

  const retriedWithoutChanges = input.stop_hook_active === true && state.failed === current;
  if (retriedWithoutChanges) {
    respond({
      systemMessage:
        "The stop check still fails on the current changes. Run `scripts/check.sh` to see the errors.",
    });
  }

  const changed = new Set(files);
  const failures = crates.map((crate) => clippy(crate, changed)).filter(Boolean);
  if (failures.length === 0) {
    writeState({ ...state, passed: current });
    process.exit(0);
  }

  writeState({ ...state, failed: current });
  const perFailure = Math.floor(outputLimit / failures.length);
  respond({
    decision: "block",
    reason: [
      "The stop check found problems in the Rust files you changed. Fix them before you finish.",
      ...failures.map((failure) => `${failure.label} failed:\n${tail(failure.output, perFailure)}`),
    ].join("\n\n"),
  });
} catch (error) {
  respond({
    systemMessage: `The stop check could not run: ${error instanceof Error ? error.message : String(error)}`,
  });
}
