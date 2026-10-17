import { ContrastIcon, SlidersHorizontalIcon } from "@hugeicons/core-free-icons";
import type { IconSvgElement } from "@hugeicons/react";

import { BandageIcon, CropIcon, Icon } from "@/components/icons";
import { useStore } from "@/hooks/use-store";
import { develop, setTool, type Tool } from "@/lib/develop/session";
import { cn } from "@/lib/utils";

const segment =
  "flex h-8 grow basis-0 items-center justify-center gap-xs rounded-s text-s font-medium transition-colors duration-(--motion-duration) ease-(--motion-easing)";

const tools: { tool: Tool; label: string; icon: IconSvgElement; shortcut: string }[] = [
  { tool: "edit", label: "Edit", icon: SlidersHorizontalIcon, shortcut: "E" },
  { tool: "crop", label: "Crop", icon: CropIcon, shortcut: "R" },
  { tool: "heal", label: "Heal", icon: BandageIcon, shortcut: "Q" },
  { tool: "masks", label: "Masks", icon: ContrastIcon, shortcut: "M" },
];

export function ToolsRow() {
  const active = useStore(develop, (state) => state.tool);
  return (
    <div
      className="flex shrink-0 gap-xxs rounded-m bg-control p-xxs"
      role="toolbar"
      aria-label="Tools"
    >
      {tools.map(({ tool, label, icon, shortcut }) => (
        <button
          key={tool}
          type="button"
          aria-pressed={active === tool}
          aria-keyshortcuts={shortcut}
          title={`${label} (${shortcut})`}
          onClick={() => setTool(active === tool && tool !== "edit" ? "edit" : tool)}
          className={cn(
            segment,
            active === tool
              ? "bg-control-active text-foreground"
              : "text-secondary-text hover:text-foreground",
          )}
        >
          <Icon icon={icon} size={15} />
          {label}
        </button>
      ))}
    </div>
  );
}
