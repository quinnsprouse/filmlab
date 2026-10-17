import { expect, test, type Page } from "@playwright/test";

import {
  exportFromDevelop,
  goldenFixture,
  openFirstPhotoInDevelop,
  openStagedRoll,
} from "./fixtures";

const goldenName = "lightcraft-golden.png";

interface PixelDiff {
  exportedSize: [number, number];
  goldenSize: [number, number];
  maxDifference: number;
  differingValues: number;
  totalValues: number;
}

async function comparePixels(page: Page): Promise<PixelDiff> {
  return page.evaluate(async (golden) => {
    const colorSpace = "srgb";

    async function pixelsOf(blob: Blob) {
      const bitmap = await createImageBitmap(blob);
      const canvas = new OffscreenCanvas(bitmap.width, bitmap.height);
      const context = canvas.getContext("2d");
      if (!context) {
        throw new Error("OffscreenCanvas has no 2d context");
      }
      context.drawImage(bitmap, 0, 0);
      const image = context.getImageData(0, 0, bitmap.width, bitmap.height, { colorSpace });
      return { width: bitmap.width, height: bitmap.height, data: image.data };
    }

    const roll = await (await navigator.storage.getDirectory()).getDirectoryHandle("roll");
    const exports = await roll.getDirectoryHandle("Exports");
    const exported = await pixelsOf(await (await exports.getFileHandle("R0000001.png")).getFile());
    const reference = await pixelsOf(await (await fetch(`/__e2e__/${golden}`)).blob());
    if (exported.data.length !== reference.data.length) {
      throw new Error(
        `Exported ${exported.width}x${exported.height} but golden is ${reference.width}x${reference.height}`,
      );
    }

    let maxDifference = 0;
    let differingValues = 0;
    for (const [index, value] of exported.data.entries()) {
      if ((index & 3) === 3) {
        continue;
      }
      const delta = Math.abs(value - (reference.data[index] ?? 0));
      maxDifference = Math.max(maxDifference, delta);
      if (delta > 0) {
        differingValues++;
      }
    }

    return {
      exportedSize: [exported.width, exported.height],
      goldenSize: [reference.width, reference.height],
      maxDifference,
      differingValues,
      totalValues: (exported.data.length / 4) * 3,
    };
  }, goldenName);
}

test("a PNG export matches LightCraft desktop's render of the same settings", async ({ page }) => {
  test.setTimeout(180_000);
  await openStagedRoll(page, { sidecar: true, served: { [goldenName]: goldenFixture } });
  await openFirstPhotoInDevelop(page);

  await exportFromDevelop(page, { format: "PNG", longEdge: 640 });
  const diff = await comparePixels(page);

  expect(diff.exportedSize).toEqual(diff.goldenSize);
  expect(diff.goldenSize).toEqual([640, 427]);
  expect(diff.maxDifference).toBeLessThanOrEqual(1);
  expect(diff.differingValues).toBeLessThanOrEqual(diff.totalValues * 0.0001);
});
