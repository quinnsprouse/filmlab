import shader from "./film.wgsl?raw";

export const usage = {
  mapRead: 0x1,
  copySrc: 0x4,
  copyDst: 0x8,
  uniform: 0x40,
  storage: 0x80,
} as const;
export const storageBindings = [1, 2, 3, 4, 5, 6, 7, 8, 9];

const computeStage = 0x4;
const writable = new Set([2, 7, 9]);
const entries = [
  "code",
  "blurVertical",
  "blurHorizontal",
  "lumaPlane",
  "reduce",
  "boxVertical",
  "boxHorizontal",
  "detail",
  "defocusVertical",
  "defocusHorizontal",
  "grainNoise",
  "grainVertical",
  "grainHorizontal",
  "softVertical",
  "grainApply",
  "dust",
  "samples",
  "finish",
  "halationLayers",
  "pyramidReduce",
  "pyramidBlur",
  "pyramidExpand",
  "pyramidAdd",
  "pyramidCombine",
];

export interface Gpu {
  device: GPUDevice;
  layout: GPUBindGroupLayout;
  pipelines: Map<string, GPUComputePipeline>;
  dummies: Map<number, GPUBuffer>;
}

export async function startGpu(): Promise<Gpu | undefined> {
  const adapter = await navigator.gpu?.requestAdapter({ powerPreference: "high-performance" });
  if (!adapter || adapter.info.isFallbackAdapter) {
    return undefined;
  }
  if (adapter.limits.maxStorageBuffersPerShaderStage < storageBindings.length) {
    return undefined;
  }
  const device = await adapter.requestDevice({
    requiredLimits: {
      maxBufferSize: adapter.limits.maxBufferSize,
      maxStorageBufferBindingSize: adapter.limits.maxStorageBufferBindingSize,
      maxStorageBuffersPerShaderStage: storageBindings.length,
    },
  });
  const layout = device.createBindGroupLayout({
    entries: [
      { binding: 0, visibility: computeStage, buffer: { type: "uniform" } },
      ...storageBindings.map((binding) => ({
        binding,
        visibility: computeStage,
        buffer: {
          type: writable.has(binding) ? ("storage" as const) : ("read-only-storage" as const),
        },
      })),
    ],
  });
  const module = device.createShaderModule({ code: shader });
  const pipelineLayout = device.createPipelineLayout({ bindGroupLayouts: [layout] });
  const built = await Promise.all(
    entries.map(
      async (entryPoint) =>
        [
          entryPoint,
          await device.createComputePipelineAsync({
            layout: pipelineLayout,
            compute: { module, entryPoint },
          }),
        ] as const,
    ),
  );
  const dummies = new Map(
    storageBindings.map((binding) => [
      binding,
      device.createBuffer({ size: 16, usage: usage.storage | usage.copyDst }),
    ]),
  );
  return { device, layout, pipelines: new Map(built), dummies };
}
