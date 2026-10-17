import { Tooltip } from "@base-ui/react/tooltip";

import {
  Alert02Icon,
  CheckmarkCircle02Icon,
  Icon,
  InformationCircleIcon,
  Loading03Icon,
} from "@/components/icons";
import { useStore } from "@/hooks/use-store";
import { library } from "@/lib/library/library";

const labels = {
  idle: undefined,
  saved: "Edits saved to folder",
  saving: "Saving…",
  readOnly: "Read-only folder · edits stay in this tab",
  failed: "Unable to save edits",
} as const;

const icons = {
  idle: undefined,
  saved: CheckmarkCircle02Icon,
  saving: Loading03Icon,
  readOnly: InformationCircleIcon,
  failed: Alert02Icon,
} as const;

export function SaveStatus() {
  const saving = useStore(library, (state) => state.saving);
  const error = useStore(library, (state) => state.saveError);
  const label = labels[saving];
  const icon = icons[saving];
  if (!label || !icon) {
    return null;
  }
  const content = (
    <output
      className={`flex items-center gap-xs text-xs font-medium ${saving === "failed" ? "text-foreground" : "text-tertiary-text"}`}
    >
      <Icon icon={icon} size={14} />
      {label}
    </output>
  );
  if (saving !== "failed" || !error) {
    return content;
  }
  return (
    <Tooltip.Provider>
      <Tooltip.Root>
        <Tooltip.Trigger
          render={
            <button type="button" aria-label="Save error details" className="cursor-default" />
          }
        >
          {content}
        </Tooltip.Trigger>
        <Tooltip.Portal>
          <Tooltip.Positioner sideOffset={8}>
            <Tooltip.Popup className="max-w-[320px] rounded-m bg-panel px-s py-xs text-xs text-secondary-text ring ring-border-strong">
              {error}
            </Tooltip.Popup>
          </Tooltip.Positioner>
        </Tooltip.Portal>
      </Tooltip.Root>
    </Tooltip.Provider>
  );
}
