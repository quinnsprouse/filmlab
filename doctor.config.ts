import type { ReactDoctorConfig } from "react-doctor/api";

import { rodeoRuleExceptions, rodeoRules } from "./lint/rodeo-rules";

export default {
  plugins: ["./lint/rules.js"],
  rules: rodeoRules,
  ignore: {
    // why: React Doctor does not read Oxlint's ignorePatterns. Without .output here, a scan after
    // `npm run build` reports issues in bundled dependencies.
    files: ["src/routeTree.gen.ts", ".output/**", "dist/**"],
    overrides: rodeoRuleExceptions,
  },
} satisfies ReactDoctorConfig;
