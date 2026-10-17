import { createFileRoute } from "@tanstack/react-router";

import { filmVisionResponse } from "@/lib/film/filmvision.server";

export const Route = createFileRoute("/filmvision/$")({
  server: {
    handlers: {
      GET: ({ request }) =>
        filmVisionResponse(new URL(request.url).pathname.slice("/filmvision/".length)),
    },
  },
});
