import type { IconSvgElement } from "@hugeicons/react";

import { Icon } from "@/components/icons";
import { cn } from "@/lib/utils";

interface IconActionProps {
  icon: IconSvgElement;
  label: string;
  onClick: () => void;
  pressed?: boolean;
  className?: string;
}

export function IconAction({ icon, label, onClick, pressed, className }: IconActionProps) {
  return (
    <button
      type="button"
      aria-label={label}
      title={label}
      aria-pressed={pressed}
      onClick={onClick}
      className={cn(
        "grid size-6 shrink-0 place-items-center rounded-s text-secondary-text transition-colors duration-(--motion-duration) ease-(--motion-easing) hover:bg-control-hover hover:text-foreground",
        pressed && "bg-control-active text-foreground",
        className,
      )}
    >
      <Icon icon={icon} size={14} />
    </button>
  );
}
