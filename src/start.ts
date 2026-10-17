import { createCsrfMiddleware, createStart } from "@tanstack/react-start";

// why: Start adds CSRF protection on its own only while this file does not exist. Once it exists,
// only the middleware listed here runs, so keep this one when you add more.
const csrfMiddleware = createCsrfMiddleware({
  // why: Page requests arrive cross-site from links and search results, so only server functions
  // are checked.
  filter: (ctx) => ctx.handlerType === "serverFn",
});

export const startInstance = createStart(() => ({
  requestMiddleware: [csrfMiddleware],
}));
