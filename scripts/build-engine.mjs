import { execFileSync } from "node:child_process";
import { existsSync, readdirSync, readFileSync, rmSync, writeFileSync } from "node:fs";
import { fileURLToPath } from "node:url";

const root = fileURLToPath(new URL("..", import.meta.url));
const engine = `${root}engine`;
const target = "wasm32-unknown-unknown";

const threadFlags = [
  "-C target-feature=+atomics,+bulk-memory,+mutable-globals",
  "-C link-arg=--shared-memory",
  "-C link-arg=--import-memory",
  "-C link-arg=--max-memory=4294967296",
  "-C link-arg=--export=__wasm_init_tls",
  "-C link-arg=--export=__tls_size",
  "-C link-arg=--export=__tls_align",
  "-C link-arg=--export=__tls_base",
].join(" ");

const builds = [
  {
    out: `${engine}/pkg`,
    wasm: `${engine}/target/${target}/release/filmlab_engine.wasm`,
    cargo: ["build", "--release", "--target", target],
    env: {},
  },
  {
    out: `${engine}/pkg-mt`,
    wasm: `${engine}/target-mt/${target}/release/filmlab_engine.wasm`,
    cargo: [
      "+nightly",
      "build",
      "--release",
      "--target",
      target,
      "--features",
      "threads",
      "-Z",
      "build-std=panic_abort,std",
      "--target-dir",
      "target-mt",
    ],
    env: { RUSTFLAGS: threadFlags },
  },
];

function run(command, args, env = {}) {
  execFileSync(command, args, { cwd: engine, stdio: "inherit", env: { ...process.env, ...env } });
}

function has(command) {
  try {
    execFileSync(command, ["--version"], { stdio: "ignore" });
    return true;
  } catch {
    return false;
  }
}

function pointWorkerHelpersAtEntry(out) {
  const snippets = `${out}/snippets`;
  if (!existsSync(snippets)) {
    return;
  }
  for (const dir of readdirSync(snippets)) {
    const helper = `${snippets}/${dir}/src/workerHelpers.js`;
    if (existsSync(helper)) {
      const source = readFileSync(helper, "utf8");
      writeFileSync(helper, source.replace("import('../../..')", "import('../../../engine.js')"));
    }
  }
}

for (const build of builds) {
  run("cargo", build.cargo, build.env);
  if (existsSync(build.out)) {
    rmSync(build.out, { recursive: true });
  }
  run("wasm-bindgen", [
    build.wasm,
    "--target",
    "web",
    "--out-dir",
    build.out,
    "--out-name",
    "engine",
  ]);
  pointWorkerHelpersAtEntry(build.out);
  if (has("wasm-opt")) {
    run("wasm-opt", [
      "-O2",
      "--enable-bulk-memory",
      "--enable-nontrapping-float-to-int",
      "--enable-sign-ext",
      "--enable-mutable-globals",
      "--enable-threads",
      `${build.out}/engine_bg.wasm`,
      "-o",
      `${build.out}/engine_bg.wasm`,
    ]);
  }
}
