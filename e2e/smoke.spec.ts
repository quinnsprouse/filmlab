import { expect, test, type Page } from "@playwright/test";

import {
  exportFromDevelop,
  openFirstPhotoInDevelop,
  openStagedRoll,
  rawStagedName,
  readRollText,
  sidecarStagedName,
} from "./fixtures";

function trackBrowserErrors(page: Page) {
  const browserErrors: Error[] = [];
  page.on("pageerror", (error) => {
    browserErrors.push(error);
  });
  page.on("console", (message) => {
    if (message.type() === "error") {
      browserErrors.push(new Error(message.text()));
    }
  });
  return browserErrors;
}

test("first run asks for a folder once the photo engine is ready", async ({ page }) => {
  const browserErrors = trackBrowserErrors(page);
  await page.goto("/");

  await expect(page.getByRole("heading", { name: "Open a folder of photos" })).toBeVisible();
  await expect(page.getByRole("button", { name: "Open folder" })).toBeEnabled();
  expect(browserErrors).toHaveLength(0);
});

test("a staged folder shows its photo, which opens in Develop with Enter", async ({ page }) => {
  const browserErrors = trackBrowserErrors(page);
  await openStagedRoll(page);

  await expect(page.getByRole("button", { name: rawStagedName })).toHaveCount(1);
  await openFirstPhotoInDevelop(page);
  await expect(page.getByLabel("Photo preview")).toBeVisible();
  expect(browserErrors).toHaveLength(0);
});

test("the d key opens the focused photo in Develop", async ({ page }) => {
  const browserErrors = trackBrowserErrors(page);
  await openStagedRoll(page);
  await openFirstPhotoInDevelop(page, "d");

  expect(browserErrors).toHaveLength(0);
});

test("an exposure nudge adds a history step and autosaves an XMP sidecar", async ({ page }) => {
  const browserErrors = trackBrowserErrors(page);
  await openStagedRoll(page);
  await openFirstPhotoInDevelop(page);

  await page.getByRole("slider", { name: "Exposure" }).focus();
  await page.keyboard.press("ArrowRight");
  await expect(
    page.getByRole("region", { name: "History" }).getByRole("button", { name: /^Exposure/ }),
  ).toBeVisible();

  await expect
    .poll(() => readRollText(page, sidecarStagedName), { timeout: 5_000 })
    .toContain("lc:settings");
  await expect
    .poll(() => readRollText(page, sidecarStagedName), { timeout: 5_000 })
    .toContain('"exposure":0.01');
  expect(browserErrors).toHaveLength(0);
});

test("export writes a JPEG at the requested long edge", async ({ page }) => {
  const browserErrors = trackBrowserErrors(page);
  await openStagedRoll(page);
  await openFirstPhotoInDevelop(page);

  await exportFromDevelop(page, { format: "JPEG", longEdge: 512 });
  const size = await page.evaluate(async () => {
    const roll = await (await navigator.storage.getDirectory()).getDirectoryHandle("roll");
    const exports = await roll.getDirectoryHandle("Exports");
    const file = await (await exports.getFileHandle("R0000001.jpg")).getFile();
    const bitmap = await createImageBitmap(file);
    return { width: bitmap.width, height: bitmap.height };
  });
  expect(size.width).toBe(512);
  expect(browserErrors).toHaveLength(0);
});

test("first run fits a phone-sized viewport without horizontal scroll", async ({ page }) => {
  const browserErrors = trackBrowserErrors(page);
  await page.setViewportSize({ width: 390, height: 844 });
  await page.goto("/");

  await expect(page.getByRole("heading", { name: "Open a folder of photos" })).toBeVisible();

  const hasHorizontalOverflow = await page.evaluate(
    () => document.documentElement.scrollWidth > document.documentElement.clientWidth,
  );
  expect(hasHorizontalOverflow).toBe(false);
  expect(browserErrors).toHaveLength(0);
});
