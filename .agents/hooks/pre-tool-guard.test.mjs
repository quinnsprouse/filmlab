import assert from "node:assert/strict";
import { spawn } from "node:child_process";
import { dirname, join, resolve } from "node:path";
import { describe, it } from "node:test";
import { fileURLToPath } from "node:url";

const hooksDir = dirname(fileURLToPath(import.meta.url));
const repoRoot = resolve(hooksDir, "..", "..");
const guard = join(hooksDir, "pre-tool-guard.mjs");

function runGuard(stdin) {
  return new Promise((resolvePromise, reject) => {
    const child = spawn(process.execPath, [guard], {
      env: { ...process.env, CLAUDE_PROJECT_DIR: repoRoot },
    });
    let stdout = "";
    child.stdout.setEncoding("utf8");
    child.stdout.on("data", (chunk) => {
      stdout += chunk;
    });
    child.on("error", reject);
    child.on("close", (status) => resolvePromise({ status, stdout }));
    child.stdin.end(stdin);
  });
}

async function decide({ toolName, toolInput }) {
  const result = await runGuard(
    JSON.stringify({ hook_event_name: "PreToolUse", tool_name: toolName, tool_input: toolInput }),
  );
  if (result.status !== 0) throw new Error(`The guard exited ${String(result.status)}`);
  if (!result.stdout.trim()) return "allow";
  const decision = JSON.parse(result.stdout)?.hookSpecificOutput?.permissionDecision;
  if (decision === "ask" || decision === "deny") return decision;
  throw new Error(`Unexpected guard output: ${result.stdout}`);
}

const bash = (command) => ({ toolName: "Bash", toolInput: { command } });
const edit = (file) => ({ toolName: "Edit", toolInput: { file_path: join(repoRoot, file) } });
const write = (file) => ({ toolName: "Write", toolInput: { file_path: join(repoRoot, file) } });
const patch = (...headers) => ({
  toolName: "apply_patch",
  toolInput: { command: ["*** Begin Patch", ...headers, "*** End Patch"].join("\n") },
});

async function expectDecisions(cases) {
  const actual = await Promise.all(cases.map(([call]) => decide(call)));
  assert.deepEqual(
    cases.map(([call], index) => [call.toolInput, actual[index]]),
    cases.map(([call, decision]) => [call.toolInput, decision]),
  );
}

describe("pre-tool guard", () => {
  it("denies new Markdown docs outside agent config and .scratch", () =>
    expectDecisions([
      [write("docs/adr/0011-use-redis.md"), "deny"],
      [write("PLAN.md"), "deny"],
      [write("native/src/README.md"), "deny"],
      [patch("*** Add File: docs/prd/export.md"), "deny"],
      [write(".scratch/plan.md"), "allow"],
      [write("CLAUDE.md"), "allow"],
      [edit("README.md"), "allow"],
      [edit("AGENTS.md"), "allow"],
    ]));

  it("denies edits to lockfiles and build output", () =>
    expectDecisions([
      [edit("native/Cargo.lock"), "deny"],
      [edit("engine/Cargo.lock"), "deny"],
      [edit("native/target/release/build/out.rs"), "deny"],
      [edit("engine/target-mt/debug/x.rs"), "deny"],
      [patch("*** Update File: native/src/gpu.rs", "*** Delete File: engine/Cargo.lock"), "deny"],
      [patch("*** Update File: native/src/a.rs", "*** Move to: native/target/a.rs"), "deny"],
    ]));

  it("allows edits to source, manifests, tracked hooks, and files outside the project", () =>
    expectDecisions([
      [edit("native/src/gpu.rs"), "allow"],
      [edit("engine/Cargo.toml"), "allow"],
      [edit(".githooks/pre-commit"), "allow"],
      [patch("*** Add File: native/src/new.rs"), "allow"],
      [{ toolName: "Write", toolInput: { file_path: "/tmp/elsewhere/notes.md" } }, "allow"],
    ]));

  it("denies hook bypasses", () =>
    expectDecisions([
      [bash('git commit -m "feat: x" --no-verify'), "deny"],
      [bash("git commit -n -m x"), "deny"],
      [bash('git commit -nm "feat: x"'), "deny"],
      [bash("git push --no-verify origin HEAD"), "deny"],
      [bash('git -c core.hooksPath=/dev/null commit -m "feat: x"'), "deny"],
      [bash("git config core.hooksPath .nowhere"), "deny"],
    ]));

  it("escalates destructive git commands to the human", () =>
    expectDecisions([
      [bash("git push --force origin main"), "ask"],
      [bash("git push -f"), "ask"],
      [bash("git push origin +main"), "ask"],
      [bash("git reset --hard HEAD~1"), "ask"],
      [bash("git checkout -- ."), "ask"],
      [bash("git clean -fd"), "ask"],
    ]));

  it("allows ordinary commands", () =>
    expectDecisions([
      [bash('git commit -m "feat: normal"'), "allow"],
      [bash("git push origin HEAD"), "allow"],
      [bash("git push --force-with-lease origin HEAD"), "allow"],
      [bash("git push -n origin HEAD"), "allow"],
      [bash("cargo build --release"), "allow"],
      [bash("scripts/check.sh"), "allow"],
    ]));

  it("never blocks on malformed input", async () => {
    const result = await runGuard("not json");
    assert.equal(result.status, 0);
    assert.equal(result.stdout, "");
  });
});
