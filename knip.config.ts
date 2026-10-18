import type { KnipConfig } from "knip";

export default {
  // why: Knip cannot see files that tools load by path or by convention: hook and script
  // commands and route files.
  entry: [
    ".agents/hooks/*.mjs",
    "e2e/**/*.ts",
    "scripts/**/*.mjs",
    "native/scripts/*.mjs",
    "src/routes/**/*.{ts,tsx}",
  ],
  ignoreDependencies: [
    // why: The commit-msg shell hook runs this executable, outside Knip's JavaScript analysis.
    "@commitlint/cli",
  ],
  // why: Only the react-doctor CLI reads doctor.config.ts, and Knip has no plugin for it.
  ignore: [
    "doctor.config.ts",
    // why: Ambient types that let `style` accept CSS custom properties; imported by no file.
    "src/components/darkroom/develop/css-properties.d.ts",
  ],
} satisfies KnipConfig;
