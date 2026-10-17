import { createRouter } from "@tanstack/react-router";

import { RouteErrorComponent } from "@/components/route-error";
import { RouteNotFoundComponent } from "@/components/route-not-found";

import { routeTree } from "./routeTree.gen";

export function getRouter() {
  const router = createRouter({
    routeTree,
    defaultPreload: "intent",
    defaultErrorComponent: RouteErrorComponent,
    defaultNotFoundComponent: RouteNotFoundComponent,
    notFoundMode: "root",
    scrollRestoration: true,
  });

  return router;
}

declare module "@tanstack/react-router" {
  interface Register {
    router: ReturnType<typeof getRouter>;
  }
}
