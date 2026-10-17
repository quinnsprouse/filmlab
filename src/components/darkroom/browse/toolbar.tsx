import { Menu } from "@base-ui/react/menu";

import { ArrowDown01Icon, Icon, Tick02Icon } from "@/components/icons";
import { cn } from "@/lib/utils";

import type { Filter, FilterCounts, Sort } from "./model";

const filters: { value: Filter; label: string }[] = [
  { value: "all", label: "All" },
  { value: "picks", label: "Picks" },
  { value: "rejected", label: "Rejected" },
];

const sorts: { value: Sort; label: string }[] = [
  { value: "newest", label: "Newest first" },
  { value: "oldest", label: "Oldest first" },
  { value: "name", label: "File name" },
];

interface ToolbarProps {
  filter: Filter;
  onFilter: (filter: Filter) => void;
  counts: FilterCounts;
  sort: Sort;
  onSort: (sort: Sort) => void;
}

function isSort(value: unknown): value is Sort {
  return sorts.some((option) => option.value === value);
}

export function Toolbar({ filter, onFilter, counts, sort, onSort }: ToolbarProps) {
  return (
    <div className="flex h-[56px] shrink-0 items-center justify-between px-l">
      <fieldset className="m-0 flex min-w-0 gap-xxs rounded-m border-0 bg-control p-xxs">
        <legend className="sr-only">Filter</legend>
        {filters.map((option) => (
          <button
            key={option.value}
            type="button"
            aria-pressed={filter === option.value}
            onClick={() => onFilter(option.value)}
            className={cn(
              "flex h-7 items-center gap-s rounded-s px-s text-s font-medium transition-colors duration-(--motion-duration) ease-(--motion-easing)",
              filter === option.value
                ? "bg-control-active text-foreground"
                : "text-secondary-text hover:bg-control-hover",
            )}
          >
            {option.label}
            <span
              className={cn(
                "font-data text-xs tabular-nums",
                filter === option.value ? "text-secondary-text" : "text-tertiary-text",
              )}
            >
              {counts[option.value]}
            </span>
          </button>
        ))}
      </fieldset>
      <Menu.Root>
        <Menu.Trigger
          aria-label="Sort photos"
          className="flex h-8 items-center gap-s rounded-m bg-control pr-xs pl-s text-s font-medium text-secondary-text transition-colors duration-(--motion-duration) ease-(--motion-easing) hover:bg-control-hover data-popup-open:bg-control-hover"
        >
          {sorts.find((option) => option.value === sort)?.label}
          <Icon icon={ArrowDown01Icon} size={14} />
        </Menu.Trigger>
        <Menu.Portal>
          <Menu.Positioner align="end" sideOffset={4}>
            <Menu.Popup className="min-w-[160px] rounded-m bg-panel p-xxs ring ring-border-strong">
              <Menu.RadioGroup
                value={sort}
                onValueChange={(value) => isSort(value) && onSort(value)}
              >
                {sorts.map((option) => (
                  <Menu.RadioItem
                    key={option.value}
                    value={option.value}
                    className="flex h-8 cursor-default items-center justify-between gap-m rounded-s px-s text-s text-foreground outline-none data-highlighted:bg-control-hover"
                  >
                    {option.label}
                    <Menu.RadioItemIndicator>
                      <Icon icon={Tick02Icon} size={14} />
                    </Menu.RadioItemIndicator>
                  </Menu.RadioItem>
                ))}
              </Menu.RadioGroup>
            </Menu.Popup>
          </Menu.Positioner>
        </Menu.Portal>
      </Menu.Root>
    </div>
  );
}
