import { AxeBuilder } from "@axe-core/playwright";
import { expect, test, type Page } from "@playwright/test";

import { openFirstPhotoInDevelop, openStagedRoll } from "./fixtures";

const wcagTags = ["wcag2a", "wcag2aa", "wcag21a", "wcag21aa", "wcag22aa"];

async function axeViolations(page: Page) {
  const results = await new AxeBuilder({ page }).withTags(wcagTags).analyze();
  return results.violations.map(({ id, help, nodes }) => ({
    id,
    help,
    targets: nodes.map((node) => node.target.join(" ")),
  }));
}

test.beforeEach(async ({ page }) => {
  // why: Axe samples colors mid-animation, so reduced motion renders the settled page.
  await page.emulateMedia({ colorScheme: "dark", reducedMotion: "reduce" });
});

test("first-run screen has no WCAG violations", async ({ page }) => {
  await page.goto("/");
  await expect(page.getByRole("heading", { name: "Open a folder of photos" })).toBeVisible();
  await expect(page.getByRole("button", { name: "Open folder" })).toBeEnabled();

  expect(await axeViolations(page)).toEqual([]);
});

test("develop view has no WCAG violations", async ({ page }) => {
  await openStagedRoll(page);
  await openFirstPhotoInDevelop(page);

  expect(await axeViolations(page)).toEqual([]);
});

test("not-found page has no WCAG violations", async ({ page }) => {
  await page.goto("/definitely-missing");
  await expect(page.getByRole("heading", { name: "Page not found" })).toBeVisible();

  expect(await axeViolations(page)).toEqual([]);
});
