export const rodeoRuleNames = [
  "no-disable-directives",
  "no-comments",
  "no-import-meta-env",
  "server-fn-requires-validator",
  "no-state-from-props",
  "no-module-scope-browser-globals",
  "no-window-navigation",
] as const;

export type RodeoRuleName = (typeof rodeoRuleNames)[number];

export const rodeoRules = Object.fromEntries(
  rodeoRuleNames.map((name) => [`rodeo/${name}`, "error" as const]),
);

export const rodeoRuleExceptions = [
  {
    // why: This module is where the app parses import.meta.env into clientEnv.
    files: ["src/config/env.ts"],
    rules: ["rodeo/no-import-meta-env"],
  },
];
