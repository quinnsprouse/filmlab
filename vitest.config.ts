import { defineConfig } from "vite-plus";

export default defineConfig({
  test: {
    environment: "node",
    // why: `vp test run --changed` follows imports, and these tests spawn their subjects as
    // processes instead, so a change to one of these paths reruns every test.
    forceRerunTriggers: [
      "**/package.json/**",
      "**/{vitest,vite}.config.*/**",
      "lint/**",
      ".agents/hooks/**",
    ],
    include: ["lint/**/*.test.ts", ".agents/hooks/*.test.ts"],
  },
});
