import type { Vec } from "./geometry";

export type Guide = "none" | "thirds" | "grid";

interface CropGuidesProps {
  width: number;
  height: number;
  corners: Vec[];
  handles: Vec[];
  guide: Guide;
}

function polygon(points: Vec[]): string {
  return points.map(([x, y]) => `${x.toFixed(2)},${y.toFixed(2)}`).join(" ");
}

function guideLines(corners: Vec[], guide: Guide): string[] {
  const [a, b, , d] = corners;
  if (!a || !b || !d || guide === "none") {
    return [];
  }
  const steps =
    guide === "thirds" ? [1 / 3, 2 / 3] : Array.from({ length: 7 }, (_, i) => (i + 1) / 8);
  const at = (u: number, v: number): Vec => [
    a[0] + (b[0] - a[0]) * u + (d[0] - a[0]) * v,
    a[1] + (b[1] - a[1]) * u + (d[1] - a[1]) * v,
  ];
  return steps.flatMap((t) => [polygon([at(t, 0), at(t, 1)]), polygon([at(0, t), at(1, t)])]);
}

function bracket(corner: Vec, toward: Vec[], length: number): string {
  const legs = toward.map(([x, y]) => {
    const dx = x - corner[0];
    const dy = y - corner[1];
    const size = Math.hypot(dx, dy) || 1;
    const reach = Math.min(length, size / 3);
    return [corner[0] + (dx / size) * reach, corner[1] + (dy / size) * reach] satisfies Vec;
  });
  const [first, second] = legs;
  return first && second ? polygon([first, corner, second]) : "";
}

function edgeBar(center: Vec, along: Vec, length: number): string {
  const size = Math.hypot(along[0], along[1]) || 1;
  const dx = ((along[0] / size) * length) / 2;
  const dy = ((along[1] / size) * length) / 2;
  return polygon([
    [center[0] - dx, center[1] - dy],
    [center[0] + dx, center[1] + dy],
  ]);
}

export function CropGuides({ width, height, corners, handles, guide }: CropGuidesProps) {
  if (corners.length !== 4) {
    return null;
  }
  const scrim = `M0 0H${width}V${height}H0Z M${corners.map(([x, y]) => `${x} ${y}`).join(" L")}Z`;
  const marks = [
    ...corners.map((corner, i) =>
      bracket(corner, [corners[(i + 1) % 4] ?? corner, corners[(i + 3) % 4] ?? corner], 18),
    ),
    ...[0, 1, 2, 3].map((i) => {
      const from = corners[i] ?? [0, 0];
      const to = corners[(i + 1) % 4] ?? from;
      return edgeBar(handles[i + 4] ?? from, [to[0] - from[0], to[1] - from[1]], 22);
    }),
  ];
  return (
    <svg
      aria-hidden="true"
      width={width}
      height={height}
      className="pointer-events-none absolute inset-0 overflow-visible"
    >
      <path d={scrim} fillRule="evenodd" className="fill-canvas/70" />
      <polygon
        points={polygon(corners)}
        fill="none"
        className="stroke-foreground/90"
        strokeWidth={1}
      />
      {guideLines(corners, guide).map((points) => (
        <polyline
          key={points}
          points={points}
          fill="none"
          className="stroke-foreground/50"
          strokeWidth={1}
        />
      ))}
      <g fill="none" className="stroke-canvas/60" strokeWidth={5}>
        {marks.map((points) => (
          <polyline key={points} points={points} />
        ))}
      </g>
      <g fill="none" className="stroke-foreground" strokeWidth={3}>
        {marks.map((points) => (
          <polyline key={points} points={points} />
        ))}
      </g>
    </svg>
  );
}
