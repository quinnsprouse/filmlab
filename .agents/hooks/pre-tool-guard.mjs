import { existsSync, readFileSync } from "node:fs";
import { dirname, isAbsolute, relative, resolve } from "node:path";
import { fileURLToPath } from "node:url";

const projectRoot =
  process.env.CLAUDE_PROJECT_DIR ?? resolve(dirname(fileURLToPath(import.meta.url)), "..", "..");

const docLocations = [".agents/", ".claude/", ".codex/", ".github/", ".scratch/"];

function isNewDoc(path) {
  return (
    /\.mdx?$/.test(path) &&
    path !== "CLAUDE.md" &&
    !docLocations.some((location) => path.startsWith(location)) &&
    !existsSync(resolve(projectRoot, path))
  );
}

const protectedPaths = [
  {
    test: (path) => path === "Cargo.lock" || path.endsWith("/Cargo.lock"),
    reason:
      "Cargo.lock is owned by Cargo. Change Cargo.toml and let cargo rewrite the lockfile on the next build.",
  },
  {
    test: isNewDoc,
    reason:
      "AGENTS.md and README.md are the only project docs. Docs cover only what code, types, and lint errors can't say. Put plans, PRDs, decision records, and notes in .scratch/, which Git ignores. To keep a decision, encode it in code, a type, or a test.",
  },
  {
    test: (path) => /^(?:engine|native)\/target[^/]*\//.test(path),
    reason: "Build output is not source. Change the input that produced this file instead.",
  },
];

const hookBypass =
  "Fix the reported problems instead of skipping the Git hooks, or ask a human to bypass them.";

const guardedCommands = [
  {
    test: /\bgit\b[^\n;&|]*\bcommit\b[^\n;&|]*\s(?:--no-verify\b|-[a-zA-Z]*n[a-zA-Z]*\b)/,
    decision: "deny",
    reason: `Committing with --no-verify skips the pre-commit and commit-msg checks. ${hookBypass}`,
  },
  {
    // why: `git push -n` is a dry run, so only the long flag skips pre-push.
    test: /\bgit\b[^\n;&|]*\bpush\b[^\n;&|]*\s--no-verify\b/,
    decision: "deny",
    reason: `Pushing with --no-verify skips the pre-push gate (scripts/check.sh). ${hookBypass}`,
  },
  {
    test: /core\.hooksPath/i,
    decision: "deny",
    reason: `Overriding core.hooksPath disables the Git hooks; scripts/setup.sh is the only thing that sets it. ${hookBypass}`,
  },
  {
    // why: --force-with-lease refuses to overwrite commits you have not fetched, so it passes.
    test: /\bgit\b[^\n;&|]*\bpush\b[^\n;&|]*\s(?:--force(?![-\w])|-[a-zA-Z]*f[a-zA-Z]*\b|\+\S)/,
    decision: "ask",
    reason:
      "A force push can overwrite commits on the remote. Use --force-with-lease, and only when a human has asked for a history rewrite.",
  },
  {
    test: /\bgit\s+(?:-\S+\s+)*(?:reset\s+--hard|checkout\s+--\s+\.|restore\s+(?:--worktree\s+)?\.|clean\s+-[a-zA-Z]*f)/,
    decision: "ask",
    reason: "This discards uncommitted work. A human should confirm destructive git commands.",
  },
];

function decide(decision, reason) {
  process.stdout.write(
    `${JSON.stringify({
      hookSpecificOutput: {
        hookEventName: "PreToolUse",
        permissionDecision: decision,
        permissionDecisionReason: reason,
      },
    })}\n`,
  );
  process.exit(0);
}

function projectPath(filePath) {
  const absolute = isAbsolute(filePath) ? filePath : resolve(projectRoot, filePath);
  const path = relative(projectRoot, absolute).replaceAll("\\", "/");
  return path.startsWith("..") ? null : path;
}

function patchTargets(patch) {
  return [
    ...patch.matchAll(/^\*\*\* (?:Add|Update|Delete) File: (.+)$|^\*\*\* Move to: (.+)$/gm),
  ].map((match) => (match[1] ?? match[2]).trim());
}

function editTargets(toolName, toolInput) {
  if (toolName === "apply_patch" && typeof toolInput?.command === "string") {
    return patchTargets(toolInput.command);
  }
  if (["Edit", "Write", "MultiEdit", "NotebookEdit"].includes(toolName)) {
    const target = toolInput?.file_path ?? toolInput?.notebook_path;
    return typeof target === "string" ? [target] : [];
  }
  return [];
}

try {
  const input = JSON.parse(readFileSync(0, "utf8"));
  if (input.hook_event_name !== "PreToolUse") process.exit(0);

  for (const target of editTargets(input.tool_name, input.tool_input)) {
    const path = projectPath(target);
    if (path === null) continue;
    const hit = protectedPaths.find((rule) => rule.test(path));
    if (hit) decide("deny", hit.reason);
  }

  if (input.tool_name === "Bash" && typeof input.tool_input?.command === "string") {
    const command = input.tool_input.command;
    const hit = guardedCommands.find((rule) => rule.test.test(command));
    if (hit) decide(hit.decision, hit.reason);
  }
} catch {
  // why: A guard that cannot parse its input must not block the agent.
}
process.exit(0);
