import { definePlugin, defineRule } from "vite-plus/lint/plugins";

const BROWSER_GLOBALS = new Set([
  "window",
  "document",
  "localStorage",
  "sessionStorage",
  "navigator",
  "location",
]);
const DISABLE_DIRECTIVE = /^\s*((?:oxlint|eslint)-disable(?:-next-line|-line)?)/;
const INITIAL_PROP = /^(?:default|initial)[A-Z_]/;
const WHY_MARKER = /^why:/;
const MIN_REASON_WORDS = 3;
const VITE_BUILT_IN_ENV = new Set(["BASE_URL", "DEV", "MODE", "PROD", "SSR"]);

function isFunction(node) {
  return (
    node.type === "FunctionDeclaration" ||
    node.type === "FunctionExpression" ||
    node.type === "ArrowFunctionExpression"
  );
}

function enclosingFunction(node) {
  let current = node.parent;
  while (current) {
    if (isFunction(current)) return current;
    current = current.parent;
  }
  return null;
}

function calleeName(callee) {
  if (callee.type === "Identifier") return callee.name;
  if (
    callee.type === "MemberExpression" &&
    !callee.computed &&
    callee.property.type === "Identifier"
  ) {
    return callee.property.name;
  }
  return null;
}

function chainMethods(call) {
  const methods = [];
  let current = call;
  while (current.type === "CallExpression") {
    const { callee } = current;
    if (
      callee.type === "MemberExpression" &&
      !callee.computed &&
      callee.property.type === "Identifier"
    ) {
      methods.push(callee.property.name);
      current = callee.object;
    } else if (callee.type === "Identifier") {
      return { root: callee.name, methods };
    } else {
      return { root: null, methods };
    }
  }
  return { root: null, methods };
}

function subtreeSome(node, predicate) {
  if (!node || typeof node.type !== "string") return false;
  if (predicate(node)) return true;
  for (const key of Object.keys(node)) {
    if (key === "parent") continue;
    const value = node[key];
    if (Array.isArray(value)) {
      if (
        value.some(
          (child) => child && typeof child.type === "string" && subtreeSome(child, predicate),
        )
      )
        return true;
    } else if (value && typeof value.type === "string" && subtreeSome(value, predicate)) {
      return true;
    }
  }
  return false;
}

const noDisableDirectives = defineRule({
  meta: {
    type: "problem",
    docs: {
      description:
        "Ban inline oxlint-disable and eslint-disable comments; exceptions belong in lint.overrides in vite.config.ts.",
    },
    messages: {
      directive:
        "Line {{line}} has an inline lint suppression ({{directive}}). Fix the code, or add a reviewed file-scoped exception under lint.overrides in vite.config.ts.",
    },
    schema: [],
  },
  create(context) {
    // why: A directive suppresses diagnostics inside its own range, including the report about
    // itself. A zero-length span at 1:0 sits outside every range.
    const fileStart = { start: { line: 1, column: 0 }, end: { line: 1, column: 0 } };
    return {
      Program() {
        for (const comment of context.sourceCode.getAllComments()) {
          const match = DISABLE_DIRECTIVE.exec(comment.value);
          if (match) {
            context.report({
              loc: fileStart,
              messageId: "directive",
              data: { line: String(comment.loc.start.line), directive: match[1] },
            });
          }
        }
      },
    };
  },
});

const serverFnRequiresValidator = defineRule({
  meta: {
    type: "problem",
    docs: {
      description:
        "A createServerFn handler that reads `data` must declare a validator so untrusted input is checked at the boundary.",
    },
    messages: {
      missing:
        "This server function reads `data` without a validator. Add .validator(schema) with a Zod schema before .handler(); see docs/agents/TANSTACK_START.md",
    },
    schema: [],
  },
  create(context) {
    return {
      CallExpression(node) {
        if (calleeName(node.callee) !== "handler") return;
        const { root, methods } = chainMethods(node);
        if (root !== "createServerFn") return;
        // why: inputValidator is the deprecated name, and no-deprecated already reports it.
        if (methods.includes("validator") || methods.includes("inputValidator")) return;
        const handler = node.arguments[0];
        if (!handler || !isFunction(handler)) return;
        const [param] = handler.params;
        if (!param) return;
        const readsData =
          (param.type === "ObjectPattern" &&
            param.properties.some(
              (property) =>
                property.type === "Property" &&
                property.key.type === "Identifier" &&
                property.key.name === "data",
            )) ||
          (param.type === "Identifier" &&
            subtreeSome(
              handler.body,
              (candidate) =>
                candidate.type === "MemberExpression" &&
                candidate.object.type === "Identifier" &&
                candidate.object.name === param.name &&
                !candidate.computed &&
                candidate.property.type === "Identifier" &&
                candidate.property.name === "data",
            ));
        if (readsData) context.report({ node: node.callee, messageId: "missing" });
      },
    };
  },
});

const noStateFromProps = defineRule({
  meta: {
    type: "problem",
    docs: {
      description:
        "useState seeded from a prop silently desyncs when the prop changes; derive the value, lift the state, or remount with key.",
    },
    messages: {
      prop: "useState is seeded from prop `{{name}}`, which will not update when the prop changes. Derive it during render, lift the state to the parent, or pass key= to remount. Props named default*/initial* are exempt. See docs/agents/REACT_PATTERNS.md",
    },
    schema: [],
  },
  create(context) {
    return {
      CallExpression(node) {
        if (calleeName(node.callee) !== "useState") return;
        const [initial] = node.arguments;
        if (!initial) return;
        const component = enclosingFunction(node);
        if (!component) return;
        const [param] = component.params;
        if (!param) return;
        let propName = null;
        if (
          initial.type === "Identifier" &&
          param.type === "ObjectPattern" &&
          param.properties.some(
            (property) =>
              property.type === "Property" &&
              property.value.type === "Identifier" &&
              property.value.name === initial.name,
          )
        ) {
          propName = initial.name;
        } else if (
          initial.type === "MemberExpression" &&
          !initial.computed &&
          initial.object.type === "Identifier" &&
          param.type === "Identifier" &&
          initial.object.name === param.name &&
          initial.property.type === "Identifier"
        ) {
          propName = initial.property.name;
        }
        if (propName && !INITIAL_PROP.test(propName)) {
          context.report({ node: initial, messageId: "prop", data: { name: propName } });
        }
      },
    };
  },
});

const noModuleScopeBrowserGlobals = defineRule({
  meta: {
    type: "problem",
    docs: {
      description:
        "Browser globals touched at module scope crash on the server during SSR; read them inside a handler, an effect, or a typeof guard.",
    },
    messages: {
      ssr: "`{{name}}` is read at module scope, which throws during server rendering. Move the read into a function that runs in the browser, or guard it with typeof {{name}} !== 'undefined'.",
    },
    schema: [],
  },
  create(context) {
    return {
      Identifier(node) {
        if (!BROWSER_GLOBALS.has(node.name)) return;
        const { parent } = node;
        if (!parent) return;
        if (parent.type === "MemberExpression" && parent.property === node && !parent.computed)
          return;
        if (parent.type === "Property" && parent.key === node && !parent.computed) return;
        if (parent.type === "UnaryExpression" && parent.operator === "typeof") return;
        if (parent.type === "ImportSpecifier" || parent.type === "ImportDefaultSpecifier") return;
        if (parent.type.startsWith("TS")) return;
        if (parent.type === "VariableDeclarator" && parent.id === node) return;
        if (enclosingFunction(node)) return;
        const scope = context.sourceCode.getScope(node);
        const declared = scope.references.some(
          (reference) => reference.identifier === node && reference.resolved !== null,
        );
        if (declared) return;
        context.report({ node, messageId: "ssr", data: { name: node.name } });
      },
    };
  },
});

function isLocation(node) {
  if (node.type === "Identifier") return node.name === "location";
  return (
    node.type === "MemberExpression" &&
    !node.computed &&
    node.object.type === "Identifier" &&
    (node.object.name === "window" || node.object.name === "globalThis") &&
    node.property.type === "Identifier" &&
    node.property.name === "location"
  );
}

const noWindowNavigation = defineRule({
  meta: {
    type: "problem",
    docs: {
      description:
        "Navigate with the router, not window.location; hard navigations drop router state, loaders, and preloading.",
    },
    messages: {
      navigate:
        "Use the router instead of window.location: useNavigate() or <Link> from @tanstack/react-router keeps loaders, search params, and preloading working. See docs/agents/TANSTACK_START.md",
    },
    schema: [],
  },
  create(context) {
    return {
      AssignmentExpression(node) {
        const target = node.left;
        if (target.type !== "MemberExpression") return;
        if (isLocation(target)) {
          context.report({ node, messageId: "navigate" });
          return;
        }
        if (
          !target.computed &&
          target.property.type === "Identifier" &&
          (target.property.name === "href" || target.property.name === "pathname") &&
          isLocation(target.object)
        ) {
          context.report({ node, messageId: "navigate" });
        }
      },
      CallExpression(node) {
        const { callee } = node;
        if (
          callee.type === "MemberExpression" &&
          !callee.computed &&
          callee.property.type === "Identifier" &&
          (callee.property.name === "assign" || callee.property.name === "replace") &&
          isLocation(callee.object)
        ) {
          context.report({ node, messageId: "navigate" });
        }
      },
    };
  },
});

function commentText(comment) {
  return comment.value
    .split("\n")
    .map((line) => line.replace(/^\s*\*?/, "").trim())
    .filter(Boolean)
    .join(" ");
}

function commentGroups(comments) {
  const groups = [];
  for (const comment of comments) {
    const previous = groups.at(-1)?.at(-1);
    const continuesLineComment =
      previous?.type === "Line" &&
      comment.type === "Line" &&
      comment.loc.start.line === previous.loc.end.line + 1;
    if (continuesLineComment) groups.at(-1).push(comment);
    else groups.push([comment]);
  }
  return groups;
}

function isGovernedElsewhere(text) {
  return DISABLE_DIRECTIVE.test(text) || text.startsWith("@ts-");
}

const noComments = defineRule({
  meta: {
    type: "problem",
    docs: {
      description:
        "Ban code comments. Agents copy comments, so a workaround note spreads. The code must say what it does. A `why:` comment is a last resort for a constraint outside the code.",
    },
    messages: {
      comment:
        "Comments are not allowed. Make the code say it: a clearer name, a smaller function, a type, or a test. Delete a comment that restates the code, and fix a workaround instead of explaining it. A `why:` comment is a last resort: use one only when the code must stay as it is because of a constraint outside your control, such as a browser bug, and name that cause. See docs/agents/LINT_RULES.md#comments",
      reason:
        "A `why:` comment needs a reason of at least {{min}} words that names the cause outside the code. If you cannot name one, delete the comment.",
    },
    schema: [],
  },
  create(context) {
    return {
      Program() {
        for (const group of commentGroups(context.sourceCode.getAllComments())) {
          const text = group.map(commentText).join(" ");
          if (isGovernedElsewhere(text)) continue;
          const loc = { start: group[0].loc.start, end: group.at(-1).loc.end };
          if (!WHY_MARKER.test(text)) {
            context.report({ loc, messageId: "comment" });
            continue;
          }
          const reasonWords = text.replace(WHY_MARKER, "").split(/\s+/).filter(Boolean).length;
          if (reasonWords < MIN_REASON_WORDS) {
            context.report({ loc, messageId: "reason", data: { min: String(MIN_REASON_WORDS) } });
          }
        }
      },
    };
  },
});

function isImportMeta(node) {
  return (
    node.type === "MetaProperty" && node.meta.name === "import" && node.property.name === "meta"
  );
}

function isPropertyNamed(member, name) {
  if (member.computed) return member.property.type === "Literal" && member.property.value === name;
  return member.property.type === "Identifier" && member.property.name === name;
}

function destructuresEnv(pattern) {
  return (
    pattern.type === "ObjectPattern" &&
    pattern.properties.some(
      (property) =>
        property.type === "Property" &&
        (property.key.type === "Identifier"
          ? property.key.name === "env"
          : property.key.type === "Literal" && property.key.value === "env"),
    )
  );
}

function readsBuiltInEnv(envMember) {
  const { parent } = envMember;
  return (
    parent?.type === "MemberExpression" &&
    parent.object === envMember &&
    !parent.computed &&
    parent.property.type === "Identifier" &&
    VITE_BUILT_IN_ENV.has(parent.property.name)
  );
}

const noImportMetaEnv = defineRule({
  meta: {
    type: "problem",
    docs: {
      description:
        "Read VITE_* variables through clientEnv, which validates them at startup, not from import.meta.env.",
    },
    messages: {
      env: "Read environment variables from clientEnv in @/config/env, which validates them when the app starts. Vite's built-in flags (DEV, PROD, SSR, MODE, BASE_URL) may be read directly. To add a variable, add it to the schema in src/config/env.ts and to .env.example.",
    },
    schema: [],
  },
  create(context) {
    return {
      MetaProperty(node) {
        if (!isImportMeta(node)) return;
        const { parent } = node;
        if (parent?.type === "MemberExpression" && parent.object === node) {
          if (isPropertyNamed(parent, "env") && !readsBuiltInEnv(parent)) {
            context.report({ node: parent, messageId: "env" });
          }
          return;
        }
        if (parent?.type === "VariableDeclarator" && destructuresEnv(parent.id)) {
          context.report({ node: parent, messageId: "env" });
        }
      },
    };
  },
});

export default definePlugin({
  meta: { name: "rodeo" },
  rules: {
    "no-disable-directives": noDisableDirectives,
    "no-comments": noComments,
    "no-import-meta-env": noImportMetaEnv,
    "server-fn-requires-validator": serverFnRequiresValidator,
    "no-state-from-props": noStateFromProps,
    "no-module-scope-browser-globals": noModuleScopeBrowserGlobals,
    "no-window-navigation": noWindowNavigation,
  },
});
