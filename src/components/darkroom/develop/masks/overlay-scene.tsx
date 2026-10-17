import type { Pt } from "./masks-model";
import type { LinearDraw, RadialDraw, Scene } from "./masks-scene";

const handleNames = ["right", "bottom", "left", "top"];
const halo = "stroke-canvas/70";
const line = "stroke-foreground";

function pathOf(points: Pt[]): string {
  return `${points.map(({ x, y }, index) => `${index === 0 ? "M" : "L"}${x},${y}`).join(" ")} Z`;
}

function Stroked({ d, width = 1, dash }: { d: string; width?: number; dash?: string }) {
  return (
    <>
      <path d={d} fill="none" strokeWidth={width + 2} className={halo} />
      <path d={d} fill="none" strokeWidth={width} strokeDasharray={dash} className={line} />
    </>
  );
}

function Handle({ at, radius = 5 }: { at: Pt; radius?: number }) {
  return (
    <>
      <circle cx={at.x} cy={at.y} r={radius + 1} fill="none" strokeWidth={3} className={halo} />
      <circle
        cx={at.x}
        cy={at.y}
        r={radius}
        className="fill-canvas/40 stroke-foreground"
        strokeWidth={1.5}
      />
    </>
  );
}

function Radial({ draw }: { draw: RadialDraw }) {
  const edge = draw.handles[0] ?? draw.rotate;
  return (
    <g>
      <Stroked d={pathOf(draw.ring)} width={1.5} />
      <Stroked d={pathOf(draw.featherRing)} dash="4 4" />
      <Stroked d={`M${edge.x},${edge.y} L${draw.rotate.x},${draw.rotate.y}`} />
      {draw.handles.map((at, position) => (
        <Handle key={handleNames[position]} at={at} />
      ))}
      <Handle at={draw.rotate} radius={4} />
    </g>
  );
}

function Linear({ draw }: { draw: LinearDraw }) {
  const dx = draw.end.x - draw.start.x;
  const dy = draw.end.y - draw.start.y;
  const length = Math.hypot(dx, dy) || 1;
  const reach = 4000;
  const px = (-dy / length) * reach;
  const py = (dx / length) * reach;
  const across = (at: Pt) => `M${at.x - px},${at.y - py} L${at.x + px},${at.y + py}`;
  return (
    <g>
      <Stroked d={across(draw.start)} />
      <Stroked d={across(draw.middle)} dash="6 6" />
      <Stroked d={across(draw.end)} />
      <Handle at={draw.start} />
      <Handle at={draw.end} />
    </g>
  );
}

export function SceneLayer({ scene }: { scene: Scene }) {
  return (
    <>
      {scene.linears.map((draw) => (
        <Linear key={`${draw.start.x}:${draw.start.y}`} draw={draw} />
      ))}
      {scene.radials.map((draw) => (
        <Radial key={`${draw.maskId}:${draw.component}`} draw={draw} />
      ))}
      {scene.pins.map((pin) => (
        <circle
          key={`${pin.maskId}:${pin.component}`}
          cx={pin.at.x}
          cy={pin.at.y}
          r={6}
          strokeWidth={1.5}
          className={
            pin.selected ? "fill-foreground stroke-canvas" : "fill-foreground/50 stroke-canvas"
          }
        />
      ))}
    </>
  );
}
