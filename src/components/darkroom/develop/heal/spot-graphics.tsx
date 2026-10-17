import { distance, type SpotShape } from "./geometry";
import type { Vec } from "./spots";

function path(points: Vec[]): string {
  return points.map((p, i) => `${i === 0 ? "M" : "L"}${p.x} ${p.y}`).join(" ");
}

interface OutlineProps {
  points: Vec[];
  radius: number;
  className: string;
}

function Outline({ points, radius, className }: OutlineProps) {
  const first = points[0];
  const last = points.at(-1);
  if (!first || !last) {
    return null;
  }
  return (
    <g className={className} fill="none">
      {points.length > 1 ? (
        <path
          d={path(points)}
          strokeWidth={radius * 2}
          strokeLinecap="round"
          strokeLinejoin="round"
          className="stroke-white/15"
        />
      ) : null}
      <circle cx={first.x} cy={first.y} r={radius} strokeWidth={1.25} className="stroke-current" />
      {points.length > 1 ? (
        <circle cx={last.x} cy={last.y} r={radius} strokeWidth={1.25} className="stroke-current" />
      ) : null}
    </g>
  );
}

function Pin({ at, selected }: { at: Vec; selected: boolean }) {
  return (
    <g>
      <circle cx={at.x} cy={at.y} r={5} className="stroke-white fill-canvas/60" strokeWidth={1.5} />
      <circle cx={at.x} cy={at.y} r={2} className={selected ? "fill-white" : "fill-white/60"} />
    </g>
  );
}

function SourceLink({ shape }: { shape: SpotShape }) {
  const target = shape.points[0];
  const source = shape.sourcePoints[0];
  if (!target || !source) {
    return null;
  }
  const gap = distance(target, source);
  if (gap < 1) {
    return null;
  }
  const unit = { x: (target.x - source.x) / gap, y: (target.y - source.y) / gap };
  const from = { x: source.x + unit.x * shape.radius, y: source.y + unit.y * shape.radius };
  const to = { x: target.x - unit.x * shape.radius, y: target.y - unit.y * shape.radius };
  const visible = gap > shape.radius * 2;
  return visible ? (
    <path d={path([from, to])} className="stroke-white/70" strokeWidth={1} strokeDasharray="4 3" />
  ) : null;
}

interface SpotGraphicsProps {
  shape: SpotShape;
  selected: boolean;
}

export function SpotGraphics({ shape, selected }: SpotGraphicsProps) {
  const target = shape.points[0];
  const source = shape.sourcePoints[0];
  return (
    <g>
      <Outline
        points={shape.points}
        radius={shape.radius}
        className={selected ? "text-white" : "text-white/60"}
      />
      {source ? (
        <>
          <Outline
            points={shape.sourcePoints}
            radius={shape.radius}
            className={selected ? "text-white/80" : "text-white/35"}
          />
          <SourceLink shape={shape} />
          <circle
            cx={source.x}
            cy={source.y}
            r={3}
            className={selected ? "fill-white" : "fill-white/50"}
          />
        </>
      ) : null}
      {target ? <Pin at={target} selected={selected} /> : null}
    </g>
  );
}
