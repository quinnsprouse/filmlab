import { useState } from "react";

import { Button } from "@/components/ui/button";

import { parseRatio } from "./aspect";
import { setAspectRatio } from "./crop-commands";

interface CustomAspectProps {
  onApply: (error: string | undefined) => void;
  onDone: () => void;
}

const field =
  "h-8 w-full min-w-0 rounded-s bg-control px-xs text-center font-data text-s tabular-nums text-foreground inset-ring inset-ring-border outline-none focus-visible:inset-ring-border-strong";

export function CustomAspect({ onApply, onDone }: CustomAspectProps) {
  const [width, setWidth] = useState("3");
  const [height, setHeight] = useState("2");
  const w = parseRatio(width);
  const h = parseRatio(height);
  const apply = () => {
    if (w === undefined || h === undefined) {
      onApply("Enter a width and a height above zero, for example 3 and 2.");
      return;
    }
    const error = setAspectRatio(w, h);
    onApply(error);
    if (!error) {
      onDone();
    }
  };
  return (
    <form
      className="flex items-center gap-xs px-s py-xs"
      onSubmit={(event) => {
        event.preventDefault();
        apply();
      }}
    >
      <input
        aria-label="Aspect width"
        inputMode="decimal"
        className={field}
        value={width}
        onChange={(event) => setWidth(event.target.value)}
      />
      <span className="text-secondary-text">:</span>
      <input
        aria-label="Aspect height"
        inputMode="decimal"
        className={field}
        value={height}
        onChange={(event) => setHeight(event.target.value)}
      />
      <Button type="submit" variant="outline">
        Apply
      </Button>
    </form>
  );
}
