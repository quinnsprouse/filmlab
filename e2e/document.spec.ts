import { expect, test } from "@playwright/test";

test("hashed assets ship compressed and cached for a year", async ({ page, request }) => {
  await page.goto("/");
  const assets = [
    await page.locator('link[rel="stylesheet"]').evaluate((link: HTMLLinkElement) => link.href),
    await page
      .locator('script[type="module"][src]')
      .evaluate((script: HTMLScriptElement) => script.src),
  ];

  const responses = await Promise.all(
    assets.map((asset) => request.get(asset, { headers: { "accept-encoding": "br, gzip" } })),
  );

  for (const response of responses) {
    expect(response.headers()["content-encoding"]).toBe("br");
    expect(response.headers()["cache-control"]).toBe("public, max-age=31536000, immutable");
  }
});

test("the photo engine's wasm ships compressed", async ({ page, request }) => {
  const engineRequest = page.waitForRequest((call) => call.url().endsWith(".wasm"));
  await page.goto("/");
  const engine = await engineRequest;

  const response = await request.get(engine.url(), {
    headers: { "accept-encoding": "br, gzip" },
  });
  expect(response.headers()["content-encoding"]).toBe("br");
});

test("the page declares one theme color for the dark theme", async ({ page }) => {
  await page.goto("/");

  await expect(page.locator('meta[name="theme-color"]')).toHaveCount(1);
  await expect(page.locator('meta[name="theme-color"]')).toHaveAttribute("content", "#0e0e0e");
});

test("each page names its own canonical URL from the configured origin", async ({ page }) => {
  await page.goto("/");

  await expect(page.locator('link[rel="canonical"]')).toHaveAttribute(
    "href",
    "https://rodeo.example/",
  );
  await expect(page.locator('meta[property="og:url"]')).toHaveAttribute(
    "content",
    "https://rodeo.example/",
  );
  await expect(page.locator('meta[property="og:image"]')).toHaveAttribute(
    "content",
    "https://rodeo.example/og-image.png",
  );
});

test("a missing page claims no canonical URL", async ({ page }) => {
  await page.goto("/definitely-missing");

  await expect(page.locator('link[rel="canonical"]')).toHaveCount(0);
  await expect(page.locator('meta[property="og:url"]')).toHaveCount(0);
  await expect(page.locator('meta[property="og:image"]')).toHaveAttribute(
    "content",
    "https://rodeo.example/og-image.png",
  );
});
