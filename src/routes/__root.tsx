import { Outlet, createRootRoute, HeadContent, Scripts, rootRouteId } from "@tanstack/react-router";
import type { ReactNode } from "react";

import { createSiteHead } from "@/config/site";

import appCss from "@/styles/app.css?url";

export const Route = createRootRoute({
  head: ({ matches }) => {
    // why: With notFoundMode "root", an unknown URL matches only the root route, so a page whose
    // deepest match is the root is a 404.
    const page = matches.at(-1);
    const hasOwnUrl = page?.status === "success" && page.routeId !== rootRouteId;
    const siteHead = createSiteHead(hasOwnUrl ? page.pathname : undefined);

    return {
      meta: [
        { charSet: "utf-8" },
        { name: "viewport", content: "width=device-width, initial-scale=1" },
        ...siteHead.meta,
      ],
      links: [
        ...siteHead.links,
        { rel: "icon", href: "/favicon.svg", type: "image/svg+xml" },
        { rel: "stylesheet", href: appCss },
      ],
    };
  },
  component: RootComponent,
  shellComponent: RootShell,
});

function RootComponent() {
  return <Outlet />;
}

function RootShell({ children }: { children: ReactNode }) {
  return (
    <html lang="en" className="dark">
      <head>
        <HeadContent />
        <meta name="theme-color" content="#0e0e0e" />
      </head>
      <body>
        <a
          href="#main"
          className="fixed top-0 left-0 z-50 -translate-y-full bg-foreground px-m py-xs text-s font-medium text-background transition-transform duration-(--motion-duration) ease-(--motion-easing) focus:translate-y-0"
        >
          Skip to content
        </a>
        <main id="main">{children}</main>
        <Scripts />
      </body>
    </html>
  );
}
