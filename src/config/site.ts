import { clientEnv } from "@/config/env";

const siteConfig = {
  name: "Filmlab",
  title: "Filmlab",
  description:
    "Develop the raw photos on your computer with film looks. Nothing is uploaded; edits are saved next to your originals.",
  ogImagePath: "/og-image.png",
} as const;

export function createSiteHead(canonicalPath: string | undefined, origin = clientEnv.VITE_APP_URL) {
  const meta: Array<Record<string, string>> = [
    { title: siteConfig.title },
    { name: "description", content: siteConfig.description },
    { property: "og:type", content: "website" },
    { property: "og:title", content: siteConfig.title },
    { property: "og:description", content: siteConfig.description },
    { name: "twitter:card", content: "summary_large_image" },
    { name: "twitter:title", content: siteConfig.title },
    { name: "twitter:description", content: siteConfig.description },
  ];
  const links: Array<Record<string, string>> = [];

  if (origin) {
    const image = new URL(siteConfig.ogImagePath, `${origin}/`).toString();
    meta.push({ property: "og:image", content: image }, { name: "twitter:image", content: image });

    if (canonicalPath !== undefined) {
      const safePathname = new URL(canonicalPath, "https://placeholder.invalid").pathname;
      const canonical = new URL(safePathname, `${origin}/`).toString();
      meta.push({ property: "og:url", content: canonical });
      links.push({ rel: "canonical", href: canonical });
    }
  }

  return { links, meta };
}
