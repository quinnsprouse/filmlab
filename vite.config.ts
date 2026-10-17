import { fileURLToPath, URL } from "node:url";

import babel from "@rolldown/plugin-babel";
import tailwindcss from "@tailwindcss/vite";
import { tanstackStart } from "@tanstack/react-start/plugin/vite";
import react, { reactCompilerPreset } from "@vitejs/plugin-react";
import { nitro } from "nitro/vite";
import { defineConfig } from "vite-plus";

import { rodeoRuleExceptions, rodeoRules } from "./lint/rodeo-rules";

// why: Vite Task's automatic file tracing needs a shared-memory channel that restricted agent
// sandboxes may not allow, so caching depends on these explicit inputs.
const verificationInputs = [
  ".agents/hooks/**",
  ".claude/settings.json",
  ".codex/hooks.json",
  ".env*",
  ".github/**",
  ".gitignore",
  ".nvmrc",
  ".vite-hooks/**",
  "AGENTS.md",
  "README.md",
  "commitlint.config.ts",
  "components.json",
  "docs/**",
  "doctor.config.ts",
  "e2e/**",
  "engine/pkg/**",
  "engine/pkg-mt/**",
  "knip.config.ts",
  "lint/**",
  "package-lock.json",
  "package.json",
  "playwright.config.ts",
  "public/**",
  "scripts/**",
  "skills-lock.json",
  "src/**",
  "tsconfig*.json",
  "vite.config.ts",
  "vitest.config.ts",
];

// why: TanStack Start hydrates with inline scripts, so the CSP has no script-src. Add one with a
// nonce before the app loads a third-party script.
const securityHeaders = {
  "content-security-policy":
    "base-uri 'self'; object-src 'none'; frame-ancestors 'none'; form-action 'self'",
  "cross-origin-opener-policy": "same-origin",
  "cross-origin-embedder-policy": "require-corp",
  "permissions-policy": "camera=(), microphone=(), geolocation=(), payment=()",
  "referrer-policy": "strict-origin-when-cross-origin",
  "strict-transport-security": "max-age=31536000",
  "x-content-type-options": "nosniff",
};

type ImportPattern = { group: string[]; message: string };

const restrictedImportPaths = ["zod", "zod/v3", "zod/v4"].map((name) => ({
  name,
  message:
    "Import schemas from zod/mini. Search schemas and client env ship in the entry chunk, and classic Zod adds about 14 KB to every route.",
}));

const restrictedImportPatterns: ImportPattern[] = [
  {
    group: ["lucide-react", "react-icons", "react-icons/*", "@heroicons/*", "@tabler/icons-react"],
    message: "Icons come from @/components/icons (Hugeicons). See docs/agents/UI_MOTION.md",
  },
  {
    group: ["framer-motion"],
    message: "Import from motion/react; framer-motion is the legacy package name.",
  },
  {
    group: ["@/routes/*"],
    message:
      "Route modules are entry points that only the router imports. Move the shared code into src/components, src/hooks, or src/lib.",
  },
];

const testApis: ImportPattern = {
  group: ["vite-plus/test", "vitest", "vitest/*", "@testing-library/*", "@playwright/test"],
  message:
    "App code has no unit tests. Test the behavior end to end in e2e/ with Playwright, through the page or through the server's HTTP endpoints. See docs/agents/TESTING.md",
};

const nodeBuiltins: ImportPattern = {
  group: ["node:*"],
  message:
    "Node built-ins exist only on the server. Use them in a *.server.ts module, which TanStack Start keeps out of the client bundle, and call that module from a server function.",
};

type ImportRestriction = [
  "error",
  { paths: typeof restrictedImportPaths; patterns: ImportPattern[] },
];

// why: Oxlint replaces a rule's options in an override instead of merging them, so each layer
// repeats the shared restrictions.
function restrictImports(...layerPatterns: ImportPattern[]): ImportRestriction {
  return [
    "error",
    { paths: restrictedImportPaths, patterns: [...restrictedImportPatterns, ...layerPatterns] },
  ];
}

const nodeGlobals = ["process", "Buffer", "global", "__dirname", "__filename", "require"].map(
  (name) => ({
    name,
    message:
      "Node globals exist only on the server. Read them in a *.server.ts module and call it from a server function. Secrets come from process.env there, parsed with a Zod Mini schema. See docs/agents/TANSTACK_START.md",
  }),
);

export default defineConfig({
  resolve: {
    alias: {
      "@": fileURLToPath(new URL("./src", import.meta.url)),
      "@engine": fileURLToPath(new URL("./engine/pkg", import.meta.url)),
      "@engine-mt": fileURLToPath(new URL("./engine/pkg-mt", import.meta.url)),
    },
    tsconfigPaths: true,
  },
  server: {
    port: 3000,
    headers: {
      "cross-origin-opener-policy": "same-origin",
      "cross-origin-embedder-policy": "require-corp",
    },
  },
  worker: {
    format: "es",
  },
  staged: {
    "*": "vp check --fix",
  },
  run: {
    tasks: {
      "verify:fast": {
        command: ["node scripts/check-toolchain.mjs", "vp check", "vp test run"],
        input: verificationInputs,
        env: ["VITE_*", "NODE_ENV"],
      },
      "verify:build": {
        command: ["tsc -b", "vp build --mode e2e --logLevel error"],
        dependsOn: ["verify:fast"],
        input: verificationInputs,
        output: [".output/**"],
        env: ["VITE_*", "NODE_ENV"],
      },
      "verify:dead-code": {
        command: "knip --reporter compact",
        dependsOn: ["verify:build"],
        input: verificationInputs,
      },
      "verify:push": {
        command: "playwright test",
        dependsOn: ["verify:dead-code"],
        cache: false,
      },
      "verify:template": {
        command: "node scripts/test-template.mjs",
        dependsOn: ["verify:push"],
        cache: false,
      },
      "verify:doctor": {
        command: "react-doctor --verbose --blocking warning",
        cache: false,
      },
      "verify:ci": {
        command: ["vp run verify:doctor", "npm audit --audit-level=high"],
        dependsOn: ["verify:template"],
        cache: false,
      },
    },
  },

  fmt: {
    tabWidth: 2,
    semi: true,
    printWidth: 100,
    singleQuote: false,
    endOfLine: "lf",
    trailingComma: "all",
    sortImports: {},
    sortTailwindcss: {
      stylesheet: "./src/styles/app.css",
      attributes: ["class", "className"],
      functions: ["clsx", "cn", "cva", "tw"],
    },
    sortPackageJson: true,
    ignorePatterns: [
      "package-lock.json",
      "routeTree.gen.ts",
      ".output",
      "dist",
      ".tanstack/",
      ".tanstack-start/",
      // why: The Graphical registry installs these files, and keeping them byte-identical to the
      // downloaded revision makes a reinstall diff show only real changes.
      "GUI.md",
      "gui/",
      ".agents/skills/graphical-*/",
      "engine/",
    ],
  },

  lint: {
    // why: Setting plugins replaces Oxlint's default set, so oxc and unicorn must stay listed.
    plugins: [
      "typescript",
      "oxc",
      "unicorn",
      "react",
      "react-perf",
      "jsx-a11y",
      "import",
      "promise",
      "node",
      "vitest",
    ],
    categories: {
      correctness: "error",
      suspicious: "warn",
      perf: "warn",
    },
    env: {
      builtin: true,
      node: true,
      browser: true,
    },
    options: {
      typeAware: true,
      typeCheck: true,
      denyWarnings: true,
      reportUnusedDisableDirectives: "error",
      respectEslintDisableDirectives: false,
    },
    jsPlugins: [
      { name: "react-hooks-js", specifier: "eslint-plugin-react-hooks" },
      { name: "playwright", specifier: "eslint-plugin-playwright" },
      { name: "shadcn", specifier: "@shadcn/lint" },
      {
        name: "eslint-tanstack-router",
        specifier: "@tanstack/eslint-plugin-router",
      },
      { name: "rodeo", specifier: "./lint/rules.js" },
    ],
    rules: {
      "no-deprecated": "warn",

      // why: The automatic JSX runtime (React 17+) does not need React in scope.
      "react/react-in-jsx-scope": "off",

      // why: Inline objects, functions, and JSX in props are normal in React 19. Motion props take
      // objects, Base UI composes through render={<Link />}, and React Compiler memoizes them.
      "react-perf/jsx-no-new-object-as-prop": "off",
      "react-perf/jsx-no-new-function-as-prop": "off",
      "react-perf/jsx-no-new-array-as-prop": "off",
      "react-perf/jsx-no-jsx-as-prop": "off",

      "import/no-unassigned-import": ["warn", { allow: ["**/*.css"] }],

      "no-restricted-imports": restrictImports(),
      "import/no-relative-parent-imports": "error",
      "import/no-default-export": "error",
      "import/no-cycle": "error",
      "unicorn/prefer-node-protocol": "error",

      "typescript/no-explicit-any": "error",
      "typescript/no-non-null-assertion": "error",
      "typescript/ban-ts-comment": [
        "error",
        { "ts-expect-error": "allow-with-description", "ts-ignore": true, "ts-nocheck": true },
      ],
      "typescript/no-misused-promises": "error",
      "typescript/prefer-promise-reject-errors": "error",
      "typescript/only-throw-error": "error",
      "typescript/use-unknown-in-catch-callback-variable": "error",
      "typescript/require-await": "error",
      "typescript/return-await": ["error", "error-handling-correctness-only"],
      "typescript/switch-exhaustiveness-check": [
        "error",
        { considerDefaultExhaustiveForUnions: false },
      ],
      "typescript/no-unsafe-argument": "error",
      "typescript/no-unsafe-assignment": "error",
      "typescript/no-unsafe-call": "error",
      "typescript/no-unsafe-member-access": "error",
      "typescript/no-unsafe-return": "error",
      "typescript/no-unsafe-type-assertion": "error",

      eqeqeq: ["error", "always", { null: "ignore" }],
      "no-empty": "error",
      "no-alert": "error",
      "no-warning-comments": [
        "error",
        { terms: ["todo", "fixme", "xxx", "hack"], location: "anywhere" },
      ],
      complexity: ["error", 20],
      "max-depth": ["error", 4],
      "react/button-has-type": "error",
      "react/checked-requires-onchange-or-readonly": "error",
      "react/no-danger": "error",
      "no-console": ["error", { allow: ["warn", "error"] }],
      "unicorn/no-array-for-each": "error",
      "unicorn/no-abusive-eslint-disable": "error",
      "vitest/no-conditional-in-test": "error",
      "vitest/no-identical-title": "error",

      ...rodeoRules,

      // why: @shadcn/lint's class grammar doesn't parse the theme's named spacing keys, so it
      // reads a placement margin such as mt-m as a restyle.
      "shadcn/no-restyle": [
        "error",
        { allow: ["layout", "m-*", "mx-*", "my-*", "mt-*", "mr-*", "mb-*", "ml-*"] },
      ],
      "shadcn/no-raw-colors": "error",
      "shadcn/no-arbitrary-values": ["error", { allow: ["layout"] }],
      "shadcn/no-inline-styles": "error",
      "shadcn/no-unknown-classes": "error",
      "shadcn/require-static-classes": "error",

      "eslint-tanstack-router/create-route-property-order": "warn",

      "react-hooks-js/rules-of-hooks": "error",
      "react-hooks-js/exhaustive-deps": "error",
      "react-hooks-js/config": "error",
      "react-hooks-js/error-boundaries": "error",
      "react-hooks-js/gating": "error",
      "react-hooks-js/globals": "error",
      "react-hooks-js/immutability": "error",
      "react-hooks-js/incompatible-library": "warn",
      "react-hooks-js/preserve-manual-memoization": "error",
      "react-hooks-js/purity": "error",
      "react-hooks-js/refs": "error",
      "react-hooks-js/set-state-in-effect": "error",
      "react-hooks-js/set-state-in-render": "error",
      "react-hooks-js/static-components": "error",
      "react-hooks-js/unsupported-syntax": "warn",
      "react-hooks-js/todo": "error",
      "react-hooks-js/use-memo": "error",
      "react-hooks-js/void-use-memo": "error",
    },
    overrides: [
      {
        files: ["**/*.{ts,tsx}"],
        rules: {
          // why: noImplicitReturns checks control flow correctly for exhaustive union switches.
          "typescript/consistent-return": "off",
          // why: valid-params matches any .catch() by name, so it rejects z.catch(schema, fallback).
          // TypeScript reports a real Promise method with the wrong arguments as TS2554.
          "promise/valid-params": "off",
        },
      },
      {
        // why: Plain JavaScript has no types, so the type-aware unsafe-* rules only add noise.
        files: ["**/*.{js,mjs,cjs}"],
        rules: {
          "typescript/no-unsafe-argument": "off",
          "typescript/no-unsafe-assignment": "off",
          "typescript/no-unsafe-call": "off",
          "typescript/no-unsafe-member-access": "off",
          "typescript/no-unsafe-return": "off",
        },
      },
      {
        // why: Config files must default-export, and Node scripts and hooks print to the terminal.
        files: ["*.config.ts", "lint/**", "scripts/**", ".agents/hooks/**", "e2e/**"],
        rules: {
          "import/no-default-export": "off",
          "no-console": "off",
          "unicorn/no-array-for-each": "off",
        },
      },
      {
        files: ["src/**"],
        rules: {
          "max-lines": ["error", { max: 300, skipBlankLines: true, skipComments: true }],
          "no-restricted-imports": restrictImports(testApis, nodeBuiltins),
          "no-restricted-globals": ["error", ...nodeGlobals],
        },
      },
      {
        files: ["src/lib/**", "src/config/**"],
        rules: {
          "no-restricted-imports": restrictImports(testApis, nodeBuiltins, {
            group: ["@/components/*", "@/hooks/*"],
            message:
              "src/lib and src/config sit below components and hooks and cannot import them. Move the shared code into src/lib, or pass it in as an argument.",
          }),
        },
      },
      {
        files: ["src/hooks/**"],
        rules: {
          "no-restricted-imports": restrictImports(testApis, nodeBuiltins, {
            group: ["@/components/*"],
            message:
              "Hooks sit below components and cannot import them. Move the shared code into src/hooks or src/lib.",
          }),
        },
      },
      {
        files: ["src/components/ui/**"],
        rules: {
          // why: The shadcn kit owns its appearance and may need structural values such as
          // ring-[3px]. no-raw-colors and no-inline-styles stay on inside it.
          "shadcn/no-restyle": "off",
          "shadcn/no-arbitrary-values": "off",
          "shadcn/require-static-classes": "off",
          "no-restricted-imports": restrictImports(testApis, nodeBuiltins, {
            group: ["@/components/*", "!@/components/ui/*", "!@/components/icons"],
            message:
              "The UI kit cannot depend on app components. Pass app content in through props or children.",
          }),
        },
      },
      {
        // why: TanStack Start keeps *.server.ts modules out of the client bundle, so they may use
        // Node built-ins and globals.
        files: ["src/**/*.server.ts"],
        rules: {
          "no-restricted-imports": restrictImports(testApis),
          "no-restricted-globals": "off",
        },
      },
      ...rodeoRuleExceptions.map(({ files, rules }) => ({
        files,
        rules: Object.fromEntries(rules.map((rule) => [rule, "off" as const])),
      })),
      {
        files: ["e2e/**/*.{ts,tsx}"],
        rules: {
          "playwright/expect-expect": "error",
          "playwright/missing-playwright-await": "error",
          "playwright/no-conditional-expect": "error",
          "playwright/no-conditional-in-test": "error",
          "playwright/no-duplicate-hooks": "error",
          "playwright/no-duplicate-slow": "error",
          "playwright/no-element-handle": "error",
          "playwright/no-eval": "error",
          "playwright/no-focused-test": "error",
          "playwright/no-force-option": "warn",
          "playwright/no-nested-step": "warn",
          "playwright/no-networkidle": "error",
          "playwright/no-page-pause": "error",
          "playwright/no-skipped-test": "warn",
          "playwright/no-standalone-expect": "error",
          "playwright/no-unsafe-references": "error",
          "playwright/no-unused-locators": "error",
          "playwright/no-useless-await": "error",
          "playwright/no-useless-not": "error",
          "playwright/no-wait-for-navigation": "error",
          "playwright/no-wait-for-selector": "error",
          "playwright/no-wait-for-timeout": "error",
          "playwright/prefer-hooks-in-order": "error",
          "playwright/prefer-hooks-on-top": "error",
          "playwright/prefer-locator": "warn",
          "playwright/prefer-to-have-count": "warn",
          "playwright/prefer-to-have-length": "warn",
          "playwright/prefer-web-first-assertions": "error",
          "playwright/valid-describe-callback": "error",
          "playwright/valid-expect": "error",
          "playwright/valid-expect-in-promise": "error",
          "playwright/valid-test-tags": "error",
          "playwright/valid-title": "error",
        },
      },
    ],
    ignorePatterns: ["dist", ".output", "routeTree.gen.ts", "engine/"],
  },

  plugins: [
    tanstackStart({ importProtection: { behavior: "error" } }),
    nitro({
      compressPublicAssets: { gzip: true, brotli: true },
      routeRules: { "/**": { headers: securityHeaders } },
    }),
    react(),
    babel({ presets: [reactCompilerPreset()] }),
    tailwindcss(),
  ],
});
