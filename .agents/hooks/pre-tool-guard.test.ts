import { spawn } from "node:child_process";
import { dirname, join, resolve } from "node:path";
import { fileURLToPath } from "node:url";

import { describe, it } from "vite-plus/test";

const hooksDir = dirname(fileURLToPath(import.meta.url));
const repoRoot = resolve(hooksDir, "..", "..");
const guard = join(hooksDir, "pre-tool-guard.mjs");

type Decision = "allow" | "ask" | "deny";
type ToolCall = { toolName: string; toolInput: Record<string, string> };

function runGuard(stdin: string) {
  return new Promise<{ status: number | null; stdout: string }>((resolvePromise, reject) => {
    const child = spawn(process.execPath, [guard], {
      env: { ...process.env, CLAUDE_PROJECT_DIR: repoRoot },
    });
    let stdout = "";
    child.stdout.setEncoding("utf8");
    child.stdout.on("data", (chunk: string) => {
      stdout += chunk;
    });
    child.on("error", reject);
    child.on("close", (status) => resolvePromise({ status, stdout }));
    child.stdin.end(stdin);
  });
}

async function decide({ toolName, toolInput }: ToolCall): Promise<Decision> {
  const result = await runGuard(
    JSON.stringify({ hook_event_name: "PreToolUse", tool_name: toolName, tool_input: toolInput }),
  );
  if (result.status !== 0) throw new Error(`The guard exited ${String(result.status)}`);
  if (!result.stdout.trim()) return "allow";
  const output: unknown = JSON.parse(result.stdout);
  if (
    typeof output === "object" &&
    output !== null &&
    "hookSpecificOutput" in output &&
    typeof output.hookSpecificOutput === "object" &&
    output.hookSpecificOutput !== null &&
    "permissionDecision" in output.hookSpecificOutput
  ) {
    const decision = output.hookSpecificOutput.permissionDecision;
    if (decision === "ask" || decision === "deny") return decision;
  }
  throw new Error(`Unexpected guard output: ${result.stdout}`);
}

const bash = (command: string): ToolCall => ({ toolName: "Bash", toolInput: { command } });
const edit = (file: string): ToolCall => ({
  toolName: "Edit",
  toolInput: { file_path: join(repoRoot, file) },
});
const patch = (...headers: string[]): ToolCall => ({
  toolName: "apply_patch",
  toolInput: { command: ["*** Begin Patch", ...headers, "*** End Patch"].join("\n") },
});

async function decisions(cases: [ToolCall, Decision][]) {
  const actual = await Promise.all(cases.map(([call]) => decide(call)));
  return {
    actual: cases.map(([call], index) => [call.toolInput, actual[index]]),
    expected: cases.map(([call, decision]) => [call.toolInput, decision]),
  };
}

describe.concurrent("pre-tool guard", { timeout: 30_000 }, () => {
  it("denies unit tests in app code", async ({ expect }) => {
    const { actual, expected } = await decisions([
      [edit("src/lib/format.test.ts"), "deny"],
      [edit("src/components/ui/snippet.spec.tsx"), "deny"],
      [patch("*** Add File: src/routes/index.test.tsx"), "deny"],
      [edit("e2e/checkout.spec.ts"), "allow"],
      [edit("lint/rules.test.ts"), "allow"],
    ]);
    expect(actual).toEqual(expected);
  });

  it("denies new Markdown docs outside docs/agents and agent config", async ({ expect }) => {
    const write = (file: string): ToolCall => ({
      toolName: "Write",
      toolInput: { file_path: join(repoRoot, file) },
    });
    const { actual, expected } = await decisions([
      [write("docs/adr/0011-use-redis.md"), "deny"],
      [write("PLAN.md"), "deny"],
      [write("src/lib/README.md"), "deny"],
      [patch("*** Add File: docs/prd/checkout.md"), "deny"],
      [write("docs/agents/DATA.md"), "allow"],
      [write("gui/themes/midnight.md"), "allow"],
      [write(".scratch/plan.md"), "allow"],
      [write("CLAUDE.md"), "allow"],
      [edit("README.md"), "allow"],
    ]);
    expect(actual).toEqual(expected);
  });

  it("denies edits to generated and tool-owned files", async ({ expect }) => {
    const { actual, expected } = await decisions([
      [edit("src/routeTree.gen.ts"), "deny"],
      [edit("package-lock.json"), "deny"],
      [edit(".vite-hooks/_/h"), "deny"],
      [edit(".output/server/index.mjs"), "deny"],
    ]);
    expect(actual).toEqual(expected);
  });

  it("allows edits to source, tracked hooks, and files outside the project", async ({ expect }) => {
    const { actual, expected } = await decisions([
      [edit("src/routes/index.tsx"), "allow"],
      [edit(".vite-hooks/pre-commit"), "allow"],
      [{ toolName: "Write", toolInput: { file_path: "/tmp/elsewhere/notes.md" } }, "allow"],
    ]);
    expect(actual).toEqual(expected);
  });

  it("denies Codex patches that touch generated or tool-owned files", async ({ expect }) => {
    const { actual, expected } = await decisions([
      [patch("*** Update File: src/routeTree.gen.ts"), "deny"],
      [
        patch("*** Update File: src/routes/index.tsx", "*** Delete File: package-lock.json"),
        "deny",
      ],
      [patch("*** Update File: src/lib/a.ts", "*** Move to: .output/a.ts"), "deny"],
      [patch("*** Add File: src/lib/new.ts"), "allow"],
    ]);
    expect(actual).toEqual(expected);
  });

  it("denies hook bypasses and foreign package managers", async ({ expect }) => {
    const { actual, expected } = await decisions([
      [bash('git commit -m "feat: x" --no-verify'), "deny"],
      [bash("git commit -n -m x"), "deny"],
      [bash('git commit -nm "feat: x"'), "deny"],
      [bash("git push --no-verify origin HEAD"), "deny"],
      [bash('git -c core.hooksPath=/dev/null commit -m "feat: x"'), "deny"],
      [bash("git config core.hooksPath .nowhere"), "deny"],
      [bash('HUSKY=0 git commit -m "feat: x"'), "deny"],
      [bash("VITE_GIT_HOOKS=0 git push"), "deny"],
      [bash("pnpm add left-pad"), "deny"],
      [bash("yarn install"), "deny"],
      [bash("npx oxlint src"), "deny"],
    ]);
    expect(actual).toEqual(expected);
  });

  it("escalates destructive git commands to the human", async ({ expect }) => {
    const { actual, expected } = await decisions([
      [bash("git push --force origin main"), "ask"],
      [bash("git push -f"), "ask"],
      [bash("git push origin +main"), "ask"],
      [bash("git reset --hard HEAD~1"), "ask"],
      [bash("git checkout -- ."), "ask"],
      [bash("git clean -fd"), "ask"],
    ]);
    expect(actual).toEqual(expected);
  });

  it("allows ordinary commands", async ({ expect }) => {
    const { actual, expected } = await decisions([
      [bash('git commit -m "feat: normal"'), "allow"],
      [bash("git push origin HEAD"), "allow"],
      [bash("git push --force-with-lease origin HEAD"), "allow"],
      [bash("git push -n origin HEAD"), "allow"],
      [bash('git commit --amend -m "feat: normal"'), "allow"],
      [bash("git push --follow-tags"), "allow"],
      [bash("npm run check"), "allow"],
      [bash("npm install"), "allow"],
      [bash("vp lint src"), "allow"],
    ]);
    expect(actual).toEqual(expected);
  });

  it("never blocks on malformed input", async ({ expect }) => {
    const result = await runGuard("not json");
    expect(result.status).toBe(0);
    expect(result.stdout).toBe("");
  });
});
