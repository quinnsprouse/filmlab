import { createHash } from "node:crypto";
import { existsSync } from "node:fs";
import { mkdir, readFile, writeFile } from "node:fs/promises";
import path from "node:path";
import { fileURLToPath } from "node:url";

import { expect, type Page } from "@playwright/test";

const here = path.dirname(fileURLToPath(import.meta.url));
const cacheDir = path.join(here, ".cache");
const rawName = "ricoh-gr3.dng";
const scratchCopy = path.join(here, "..", ".scratch", "fixtures", rawName);
const rawUrl =
  "https://raw.pixls.us/getfile.php/3115/nice/Ricoh%20-%20GR%20III%20-%2014bit%20%283:2%29.DNG";
const rawSha256 = "05513ee72f7cac4165534c9f64995412084f414a033c80c473c79fd94154292e";

export const sidecarFixture = path.join(here, "fixtures", "ricoh-gr3.xmp");
export const goldenFixture = path.join(here, "fixtures", "lightcraft-golden.png");

export const rawStagedName = "R0000001.DNG";
export const sidecarStagedName = "R0000001.xmp";

function sha256(bytes: Uint8Array): string {
  return createHash("sha256").update(bytes).digest("hex");
}

async function download(): Promise<Buffer> {
  const response = await fetch(rawUrl);
  if (!response.ok) {
    throw new Error(`Downloading ${rawUrl} failed with HTTP ${response.status}`);
  }
  return Buffer.from(await response.arrayBuffer());
}

async function rawBytes(): Promise<{ bytes: Buffer; source: string }> {
  if (existsSync(scratchCopy)) {
    return { bytes: await readFile(scratchCopy), source: scratchCopy };
  }
  return { bytes: await download(), source: rawUrl };
}

export async function rawFixturePath(): Promise<string> {
  const cached = path.join(cacheDir, rawName);
  if (existsSync(cached) && sha256(await readFile(cached)) === rawSha256) {
    return cached;
  }
  const { bytes, source } = await rawBytes();
  const actual = sha256(bytes);
  if (actual !== rawSha256) {
    throw new Error(
      `The raw fixture from ${source} has sha256 ${actual}, expected ${rawSha256}. Replace the file or update rawSha256 in e2e/fixtures.ts.`,
    );
  }
  await mkdir(cacheDir, { recursive: true });
  await writeFile(cached, bytes);
  return cached;
}

export async function serveFixtures(page: Page, files: Record<string, string>) {
  await page.route("**/__e2e__/*", async (route) => {
    const name = decodeURIComponent(new URL(route.request().url()).pathname.split("/").pop() ?? "");
    const local = files[name];
    if (local === undefined) {
      await route.fulfill({ status: 404 });
      return;
    }
    await route.fulfill({ body: await readFile(local), contentType: "application/octet-stream" });
  });
}

export async function stubFolderPicker(page: Page) {
  await page.addInitScript(() => {
    Object.defineProperty(window, "showDirectoryPicker", {
      configurable: true,
      value: async () => (await navigator.storage.getDirectory()).getDirectoryHandle("roll"),
    });
  });
}

export async function stageRoll(page: Page, names: string[]) {
  await page.evaluate(async (fileNames) => {
    const root = await navigator.storage.getDirectory();
    await root.removeEntry("roll", { recursive: true }).catch(() => undefined);
    const roll = await root.getDirectoryHandle("roll", { create: true });
    await Promise.all(
      fileNames.map(async (name) => {
        const bytes = await (await fetch(`/__e2e__/${encodeURIComponent(name)}`)).arrayBuffer();
        const handle = await roll.getFileHandle(name, { create: true });
        const writable = await handle.createWritable();
        await writable.write(bytes);
        await writable.close();
      }),
    );
  }, names);
}

export async function readRollText(page: Page, name: string): Promise<string | undefined> {
  return page.evaluate(async (fileName) => {
    const roll = await (await navigator.storage.getDirectory()).getDirectoryHandle("roll");
    try {
      return await (await (await roll.getFileHandle(fileName)).getFile()).text();
    } catch {
      return undefined;
    }
  }, name);
}

export async function openStagedRoll(
  page: Page,
  { sidecar = false, served = {} }: { sidecar?: boolean; served?: Record<string, string> } = {},
) {
  const staged: Record<string, string> = { [rawStagedName]: await rawFixturePath() };
  if (sidecar) {
    staged[sidecarStagedName] = sidecarFixture;
  }
  await stubFolderPicker(page);
  await serveFixtures(page, { ...served, ...staged });
  await page.goto("/");
  await stageRoll(page, Object.keys(staged));
  await page.getByRole("button", { name: "Open folder" }).click();
  await expect(page.getByRole("button", { name: rawStagedName })).toBeVisible({ timeout: 20_000 });
}

export async function openFirstPhotoInDevelop(page: Page, key: "Enter" | "d" = "Enter") {
  await page.getByRole("button", { name: rawStagedName }).click();
  await expect(page.getByRole("region", { name: "Info" })).toContainText("6000 × 4000 px", {
    timeout: 20_000,
  });
  await page.keyboard.press(key);
  await expect(page.getByLabel("Photo preview")).toBeVisible({ timeout: 20_000 });
}

export async function exportFromDevelop(
  page: Page,
  { format, longEdge }: { format: "JPEG" | "PNG"; longEdge: number },
) {
  await page.getByRole("button", { name: "Export", exact: true }).click();
  const dialog = page.getByRole("dialog", { name: "Export 1 photo" });
  await dialog.getByText(format, { exact: true }).click();
  await dialog.getByText("Long edge", { exact: true }).click();
  await dialog.getByRole("spinbutton", { name: "Long edge in pixels" }).fill(String(longEdge));
  await dialog.getByRole("button", { name: "Export 1 photo" }).click();
  await expect(dialog.getByText("Exported 1 photo to Exports")).toBeVisible({ timeout: 120_000 });
}
