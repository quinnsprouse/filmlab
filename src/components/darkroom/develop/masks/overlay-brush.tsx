import type { Pt } from "./masks-model";

interface BrushLayerProps {
  cursor: Pt | undefined;
  stroke: Pt[] | undefined;
  radius: number;
  feather: number;
  erasing: boolean;
}

export function BrushLayer({ cursor, stroke, radius, feather, erasing }: BrushLayerProps) {
  const inner = radius * Math.max(0.05, 1 - feather / 100);
  const dash = erasing ? "3 3" : undefined;
  return (
    <>
      {stroke && stroke.length > 0 ? (
        <polyline
          points={stroke.map(({ x, y }) => `${x},${y}`).join(" ")}
          fill="none"
          strokeLinecap="round"
          strokeLinejoin="round"
          strokeWidth={Math.max(1, radius * 2)}
          className={erasing ? "stroke-foreground/20" : "stroke-destructive/35"}
        />
      ) : null}
      {cursor ? (
        <g>
          <circle
            cx={cursor.x}
            cy={cursor.y}
            r={radius}
            fill="none"
            strokeWidth={3}
            className="stroke-canvas/70"
          />
          <circle
            cx={cursor.x}
            cy={cursor.y}
            r={radius}
            fill="none"
            strokeWidth={1}
            strokeDasharray={dash}
            className="stroke-foreground"
          />
          <circle
            cx={cursor.x}
            cy={cursor.y}
            r={inner}
            fill="none"
            strokeWidth={1}
            className="stroke-foreground/60"
          />
        </g>
      ) : null}
    </>
  );
}
