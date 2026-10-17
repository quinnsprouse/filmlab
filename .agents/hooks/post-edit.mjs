import { spawnSync } from "node:child_process";
import { existsSync, lstatSync, readFileSync, realpathSync } from "node:fs";
import { createRequire } from "node:module";
import { dirname, extname, isAbsolute, relative, resolve } from "node:path";
import { fileURLToPath } from "node:url";

const require = createRequire(import.meta.url);
const fallbackRoot = dirname(dirname(dirname(fileURLToPath(import.meta.url))));
const projectRoot = realpathSync(process.env.CLAUDE_PROJECT_DIR ?? fallbackRoot);
const supportedExtensions = new Set([
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
const typeScriptExtensions = new Set([".cts", ".mts", ".ts", ".tsx"]);
const lintExtensions = new Set([".cjs", ".cts", ".js", ".jsx", ".mjs", ".mts", ".ts", ".tsx"]);
const ignoredRoots = [
  ".git",
  ".output",
  ".tanstack",
  ".tanstack-start",
  "coverage",
  "node_modules",
];

function packageBinary(packageName, binaryName) {
  const packagePath = require.resolve(`${packageName}/package.json`);
  const packageJson = JSON.parse(readFileSync(packagePath, "utf8"));
  const binary =
    typeof packageJson.bin === "string" ? packageJson.bin : packageJson.bin?.[binaryName];
  if (!binary) throw new Error(`${packageName} does not expose ${binaryName}`);
  return resolve(dirname(packagePath), binary);
}

function run(binary, args) {
  return spawnSync(process.execPath, [binary, ...args], {
    cwd: projectRoot,
    encoding: "utf8",
    env: { ...process.env, NO_COLOR: "1" },
  });
}

function report(message) {
  const additionalContext = message.length > 4_000 ? `${message.slice(0, 3_997)}...` : message;
  process.stdout.write(
    `${JSON.stringify({ hookSpecificOutput: { hookEventName: "PostToolUse", additionalContext } })}\n`,
  );
}

function editedFiles(toolName, toolInput) {
  if (toolName === "apply_patch" && typeof toolInput?.command === "string") {
    return [
      ...toolInput.command.matchAll(/^\*\*\* (?:Add|Update) File: (.+)$|^\*\*\* Move to: (.+)$/gm),
    ].map((match) => (match[1] ?? match[2]).trim());
  }
  if (
    ["Edit", "Write", "MultiEdit"].includes(toolName) &&
    typeof toolInput?.file_path === "string"
  ) {
    return [toolInput.file_path];
  }
  return [];
}

function projectFile(filePath) {
  const candidate = isAbsolute(filePath) ? filePath : resolve(projectRoot, filePath);
  if (!existsSync(candidate) || !lstatSync(candidate).isFile()) return null;

  const target = realpathSync(candidate);
  const projectPath = relative(projectRoot, target);
  if (
    projectPath === "" ||
    projectPath.startsWith("..") ||
    isAbsolute(projectPath) ||
    ignoredRoots.some((root) => projectPath === root || projectPath.startsWith(`${root}/`)) ||
    !supportedExtensions.has(extname(target).toLowerCase())
  ) {
    return null;
  }
  return { target, projectPath };
}

try {
  const input = JSON.parse(readFileSync(0, "utf8"));
  if (input.hook_event_name !== "PostToolUse") process.exit(0);

  const files = editedFiles(input.tool_name, input.tool_input)
    .map(projectFile)
    .filter((file) => file !== null);
  if (files.length === 0) process.exit(0);

  const vp = packageBinary("vite-plus", "vp");
  const feedback = [];

  for (const { target, projectPath } of files) {
    const format = run(vp, ["fmt", target]);
    if (format.status !== 0) {
      feedback.push(
        `Automatic formatting failed for ${projectPath}.\n${format.stderr || format.stdout}`,
      );
      continue;
    }

    if (lintExtensions.has(extname(target).toLowerCase())) {
      const lint = run(vp, ["lint", target]);
      if (lint.status !== 0) {
        feedback.push(`Lint found an issue in ${projectPath}.\n${lint.stdout || lint.stderr}`);
      }
    }
  }

  const typeScriptFile = files.find(({ target }) =>
    typeScriptExtensions.has(extname(target).toLowerCase()),
  );
  if (typeScriptFile) {
    const typecheck = run(packageBinary("typescript", "tsc"), ["-b", "--pretty", "false"]);
    if (typecheck.status !== 0) {
      feedback.push(
        `Type checking found an issue after editing ${typeScriptFile.projectPath}.\n${typecheck.stdout || typecheck.stderr}`,
      );
    }
  }

  if (feedback.length > 0) report(feedback.join("\n\n"));
} catch (error) {
  report(
    `Post-edit feedback could not run.\n${error instanceof Error ? error.message : String(error)}`,
  );
}
