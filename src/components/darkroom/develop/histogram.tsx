import { useStore } from "@/hooks/use-store";
import { develop } from "@/lib/develop/session";

const bins = 256;
const width = 272;
const height = 72;
const channels = [
  { offset: 0, fill: "rgb(255 90 80 / 55%)" },
  { offset: bins, fill: "rgb(110 220 120 / 50%)" },
  { offset: bins * 2, fill: "rgb(90 150 255 / 55%)" },
];

function at(counts: Uint32Array, index: number): number {
  return counts[index] ?? 0;
}

function robustMax(counts: Uint32Array): number {
  const inner: number[] = [];
  for (const { offset } of channels) {
    for (let bin = 1; bin < bins - 1; bin += 1) {
      inner.push(at(counts, offset + bin));
    }
  }
  inner.sort((a, b) => a - b);
  return Math.max(1, inner[Math.floor(inner.length * 0.995)] ?? 1);
}

function areaPath(counts: Uint32Array, offset: number, peak: number): string {
  const points = Array.from({ length: bins }, (_, bin) => {
    const x = (bin / (bins - 1)) * width;
    const y = height - Math.min(1, at(counts, offset + bin) / peak) * (height - 4);
    return `L${x.toFixed(1)} ${y.toFixed(1)}`;
  });
  return `M0 ${height} ${points.join(" ")} L${width} ${height} Z`;
}

function shares(counts: Uint32Array): [number, number, number] {
  const luma = bins * 3;
  let dark = 0;
  let mid = 0;
  let light = 0;
  for (let bin = 0; bin < bins; bin += 1) {
    const count = at(counts, luma + bin);
    if (bin < 64) {
      dark += count;
    } else if (bin < 192) {
      mid += count;
    } else {
      light += count;
    }
  }
  const total = Math.max(1, dark + mid + light);
  return [dark / total, mid / total, light / total];
}

function Readout({ label, share }: { label: string; share: number }) {
  return (
    <div className="flex items-baseline gap-xs">
      <span className="text-xs font-medium text-tertiary-text">{label}</span>
      <span className="font-data text-xs font-medium text-secondary-text tabular-nums">
        {Math.round(share * 100)}%
      </span>
    </div>
  );
}

export function Histogram() {
  const histogram = useStore(develop, (state) => state.frame?.histogram);
  const counts = histogram?.subarray(0, bins * 4);
  const peak = counts ? robustMax(counts) : 1;
  const [dark, mid, light] = counts ? shares(counts) : [0, 0, 0];

  return (
    <div className="flex shrink-0 flex-col gap-xs rounded-l bg-panel p-xs ring ring-border">
      <svg
        aria-label="Histogram"
        viewBox={`0 0 ${width} ${height}`}
        preserveAspectRatio="none"
        className="h-20 w-full rounded-xs bg-control"
      >
        {counts
          ? channels.map(({ offset, fill }) => (
              <path
                key={offset}
                d={areaPath(counts, offset, peak)}
                fill={fill}
                className="mix-blend-screen"
              />
            ))
          : null}
      </svg>
      <div className="flex items-center justify-between px-xxs">
        <Readout label="Shadows" share={dark} />
        <Readout label="Mid" share={mid} />
        <Readout label="Highlights" share={light} />
      </div>
    </div>
  );
}
