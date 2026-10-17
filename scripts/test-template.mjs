import assert from "node:assert/strict";
import { spawn, spawnSync } from "node:child_process";
import {
  appendFileSync,
  cpSync,
  existsSync,
  mkdirSync,
  mkdtempSync,
  readFileSync,
  rmSync,
  writeFileSync,
} from "node:fs";
import { createServer } from "node:net";
import { tmpdir } from "node:os";
import { dirname, join } from "node:path";
import { fileURLToPath } from "node:url";

const repoRoot = dirname(fileURLToPath(new URL("../package.json", import.meta.url)));
const appRoot = mkdtempSync(join(tmpdir(), "rodeo-template-"));
const artifactRoot = join(repoRoot, "test-results", "starter-journey");
const journeyLog = join(artifactRoot, "journey.log");
const keepTemplate = process.env.KEEP_TEMPLATE_TEST === "1";
const npmExecPath = process.env.npm_execpath;
let currentCommand = "initialization";
let productionServer;
let succeeded = false;

rmSync(artifactRoot, { force: true, recursive: true });
mkdirSync(artifactRoot, { recursive: true });

function log(message) {
  appendFileSync(journeyLog, `${message}\n`);
}

function run(command, args, options = {}) {
  currentCommand = `${command} ${args.join(" ")}`;
  log(`\n$ ${currentCommand}`);
  const cwd = options.cwd ?? appRoot;
  const env = { ...process.env, CI: "1" };
  if (cwd !== repoRoot) {
    delete env.GIT_INDEX_FILE;
    // why: In an agent session this points at the source checkout, and the hooks must use the
    // temporary app.
    delete env.CLAUDE_PROJECT_DIR;
  }
  const result = spawnSync(command, args, {
    cwd,
    encoding: "utf8",
    env,
    input: options.input,
    maxBuffer: 50 * 1024 * 1024,
  });

  if (result.error) throw result.error;

  const output = [result.stdout, result.stderr].filter(Boolean).join("");
  if (output) {
    log(output.trimEnd());
    if (!options.capture) process.stdout.write(output);
  }
  const failure = `${currentCommand} exited ${result.status}${output ? `\n${output}` : ""}`;

  if (options.expectStatus !== undefined) {
    assert.equal(result.status, options.expectStatus, failure);
  } else if (result.status !== 0) {
    throw new Error(failure);
  }

  return result;
}

function writeTree() {
  return run("git", ["write-tree"], { capture: true }).stdout.trim();
}

function runNpm(args, options) {
  return npmExecPath
    ? run(process.execPath, [npmExecPath, ...args], options)
    : run("npm", args, options);
}

function assertTree(expected, label) {
  run("git", ["add", "--all"]);
  assert.equal(writeTree(), expected, `${label} changed the distributed template tree`);
}

async function reservePort() {
  const server = createServer();
  await new Promise((resolvePromise, reject) => {
    server.once("error", reject);
    server.listen(0, "127.0.0.1", resolvePromise);
  });
  const address = server.address();
  assert(address && typeof address === "object");
  await new Promise((resolvePromise) => server.close(resolvePromise));
  return address.port;
}

async function smokeProductionServer() {
  currentCommand = "boot production Nitro server";
  const port = await reservePort();
  const serverLog = join(artifactRoot, "production-server.log");
  productionServer = spawn(process.execPath, [".output/server/index.mjs"], {
    cwd: appRoot,
    env: {
      ...process.env,
      CI: "1",
      NODE_ENV: "production",
      NITRO_HOST: "127.0.0.1",
      NITRO_PORT: String(port),
    },
    stdio: ["ignore", "pipe", "pipe"],
  });
  productionServer.stdout.on("data", (chunk) => appendFileSync(serverLog, chunk));
  productionServer.stderr.on("data", (chunk) => appendFileSync(serverLog, chunk));

  await waitForHealthyServer(`http://127.0.0.1:${port}/`, Date.now() + 20_000);
}

async function waitForHealthyServer(url, deadline) {
  try {
    const response = await fetch(url);
    const html = await response.text();
    assert.equal(response.status, 200);
    assert.match(html, /Route loader and server function are connected\./);
  } catch (error) {
    if (Date.now() >= deadline) {
      throw new Error(`Production server did not become healthy: ${String(error)}`, {
        cause: error,
      });
    }
    await new Promise((resolvePromise) => setTimeout(resolvePromise, 200));
    await waitForHealthyServer(url, deadline);
  }
}

function collectFailureEvidence(error) {
  const status = spawnSync("git", ["status", "--porcelain=v1"], {
    cwd: appRoot,
    encoding: "utf8",
  });
  writeFileSync(
    join(artifactRoot, "failure.txt"),
    [
      `Failed command: ${currentCommand}`,
      `Temporary app: ${appRoot}`,
      `Node: ${process.version}`,
      `Platform: ${process.platform} ${process.arch}`,
      "",
      error instanceof Error ? error.stack : String(error),
      "",
      "git status --porcelain=v1",
      status.stdout || status.stderr || "(no output)",
    ].join("\n"),
  );

  for (const directory of ["test-results", "playwright-report"]) {
    const source = join(appRoot, directory);
    if (existsSync(source)) {
      cpSync(source, join(artifactRoot, directory), { recursive: true });
    }
  }
}

try {
  // why: Users install with degit, which copies tracked files and symlinks without .git.
  run("git", ["checkout-index", "--all", "--force", `--prefix=${appRoot}/`], {
    cwd: repoRoot,
  });
  assert.equal(existsSync(join(appRoot, ".git")), false);

  const skippedSetup = run(process.execPath, ["scripts/setup.mjs"], { capture: true });
  assert.match(skippedSetup.stdout, /Git hooks skipped/);

  run("git", ["init", "--initial-branch=main", "--quiet"]);
  run("git", ["add", "--all"]);
  const distributedTree = writeTree();

  runNpm(["ci", "--no-audit", "--no-fund"]);
  assertTree(distributedTree, "npm ci");

  const hooksPath = run("git", ["config", "--get", "core.hooksPath"], {
    capture: true,
  }).stdout.trim();
  assert.equal(hooksPath, ".vite-hooks/_");
  assert.equal(existsSync(join(appRoot, ".vite-hooks", "_", "h")), true);
  assert.equal(existsSync(join(appRoot, ".husky")), false);

  runNpm(["run", "setup"]);
  run("git", ["hook", "run", "pre-commit"]);
  assertTree(distributedTree, "pre-commit feedback");

  const claudeSettings = JSON.parse(readFileSync(join(appRoot, ".claude/settings.json"), "utf8"));
  assert.equal(claudeSettings.hooks.PostToolUse[0].matcher, "Edit|Write|MultiEdit");
  assert.equal(
    claudeSettings.hooks.PreToolUse[0].matcher,
    "Edit|Write|MultiEdit|NotebookEdit|Bash",
  );
  const codexHooks = JSON.parse(readFileSync(join(appRoot, ".codex/hooks.json"), "utf8"));
  assert.match(codexHooks.hooks.PreToolUse[0].matcher, /\bapply_patch\b/);
  assert.match(codexHooks.hooks.PostToolUse[0].matcher, /\bapply_patch\b/);
  const codexGuard = run("sh", ["-c", codexHooks.hooks.PreToolUse[0].hooks[0].command], {
    capture: true,
    input: JSON.stringify({
      hook_event_name: "PreToolUse",
      tool_name: "apply_patch",
      tool_input: { command: "*** Begin Patch\n*** Update File: package-lock.json\n*** End Patch" },
    }),
  });
  assert.match(codexGuard.stdout, /"permissionDecision":"deny"/);

  const hookProbe = join(appRoot, ".scratch", "hook-probe.ts");
  mkdirSync(dirname(hookProbe), { recursive: true });
  writeFileSync(hookProbe, "export  const hookProbe={value:'ok'}\n");
  run(process.execPath, [".agents/hooks/post-edit.mjs"], {
    capture: true,
    input: JSON.stringify({
      hook_event_name: "PostToolUse",
      tool_name: "Write",
      tool_input: { file_path: hookProbe },
    }),
  });
  assert.equal(readFileSync(hookProbe, "utf8"), 'export const hookProbe = { value: "ok" };\n');
  rmSync(hookProbe);

  const lintProbe = join(appRoot, "src", "lint-probe.tsx");
  writeFileSync(
    lintProbe,
    'import { useEffect } from "react";\nexport function Probe({ title }: { title: string }) {\n  useEffect(() => { document.title = title; }, []);\n  return null;\n}\n',
  );
  const lintFeedback = run(process.execPath, [".agents/hooks/post-edit.mjs"], {
    capture: true,
    input: JSON.stringify({
      hook_event_name: "PostToolUse",
      tool_name: "Write",
      tool_input: { file_path: lintProbe },
    }),
  });
  assert.match(lintFeedback.stdout, /react-hooks-js\(exhaustive-deps\)/);
  rmSync(lintProbe);

  const guardDecision = (command) =>
    run(process.execPath, [".agents/hooks/pre-tool-guard.mjs"], {
      capture: true,
      input: JSON.stringify({
        hook_event_name: "PreToolUse",
        tool_name: "Bash",
        tool_input: { command },
      }),
    }).stdout;
  assert.match(guardDecision('git commit -m "feat: x" --no-verify'), /"permissionDecision":"deny"/);
  assert.equal(guardDecision("npm run check"), "");

  run("git", [
    "-c",
    "user.name=Rodeo CI",
    "-c",
    "user.email=ci@localhost",
    "commit",
    "--no-verify",
    "--quiet",
    "-m",
    "chore: establish template baseline",
  ]);
  assert.equal(run("git", ["status", "--porcelain=v1"], { capture: true }).stdout, "");

  const stopCheck = (stopHookActive) =>
    run(process.execPath, [".agents/hooks/stop-check.mjs"], {
      capture: true,
      input: JSON.stringify({ hook_event_name: "Stop", stop_hook_active: stopHookActive }),
    }).stdout;
  assert.equal(stopCheck(false), "");
  const stopProbe = join(appRoot, "src", "stop-probe.ts");
  writeFileSync(stopProbe, "export function probe(value: any) {\n  return value;\n}\n");
  assert.match(stopCheck(false), /"decision":"block".*no-explicit-any/s);
  assert.match(stopCheck(true), /"systemMessage"/);
  rmSync(stopProbe);
  assert.equal(stopCheck(false), "");

  const commitMessagePath = join(appRoot, ".git", "COMMIT_EDITMSG");
  writeFileSync(commitMessagePath, "chore: verify template journey\n");
  run("git", ["hook", "run", "commit-msg", "--", commitMessagePath]);

  writeFileSync(commitMessagePath, "invalid message\n");
  run("git", ["hook", "run", "commit-msg", "--", commitMessagePath], {
    capture: true,
    expectStatus: 1,
  });

  for (const ignoredPath of [
    ".claude/settings.local.json",
    ".scratch/probe.txt",
    "tmp/probe.txt",
    "src/example/__pycache__/probe.pyc",
  ]) {
    run("git", ["check-ignore", "--no-index", "--quiet", "--", ignoredPath]);
  }

  runNpm(["run", "test:e2e:install"]);

  run("git", ["hook", "run", "pre-push"]);
  assert.equal(existsSync(join(appRoot, ".output", "server", "index.mjs")), true);
  await smokeProductionServer();
  assert.equal(run("git", ["status", "--porcelain=v1"], { capture: true }).stdout, "");

  succeeded = true;
  console.log(`Template journey passed in ${appRoot}`);
} catch (error) {
  collectFailureEvidence(error);
  throw error;
} finally {
  if (productionServer && !productionServer.killed) productionServer.kill("SIGTERM");
  if (keepTemplate || !succeeded) {
    console.log(`Preserved template at ${appRoot}`);
    if (!succeeded) console.log(`Failure evidence: ${artifactRoot}`);
  } else {
    rmSync(appRoot, { force: true, recursive: true });
    rmSync(artifactRoot, { force: true, recursive: true });
  }
}
