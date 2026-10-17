import { Menu } from "@base-ui/react/menu";
import {
  ContrastIcon,
  Delete02Icon,
  MinusSignIcon,
  PlusSignIcon,
} from "@hugeicons/core-free-icons";

import { Icon } from "@/components/icons";

import { IconAction } from "./icon-action";
import { KindMenu } from "./kind-menu";
import { addComponent, editComponent } from "./masks-commands";
import { componentName, type Mask, type MaskOp } from "./masks-model";
import { RangeControls } from "./range-controls";

const ops: { op: MaskOp; label: string }[] = [
  { op: "add", label: "Add" },
  { op: "subtract", label: "Subtract" },
  { op: "intersect", label: "Intersect" },
];

const opSigns: Record<MaskOp, string> = { add: "+", subtract: "−", intersect: "∩" };

function OpMenu({ mask, index }: { mask: Mask; index: number }) {
  const current = mask.components[index]?.op ?? "add";
  return (
    <Menu.Root>
      <Menu.Trigger
        aria-label={`Combine mode: ${current}`}
        className="grid size-6 shrink-0 place-items-center rounded-s bg-control text-s font-heavy text-secondary-text hover:bg-control-hover hover:text-foreground data-popup-open:bg-control-hover"
      >
        {opSigns[current]}
      </Menu.Trigger>
      <Menu.Portal>
        <Menu.Positioner sideOffset={4} align="start">
          <Menu.Popup className="min-w-[132px] rounded-m bg-panel p-xxs ring ring-border-strong">
            {ops.map(({ op, label }) => (
              <Menu.Item
                key={op}
                onClick={() => editComponent(mask.id, index, "op", op)}
                className="flex h-8 cursor-default items-center gap-s rounded-s px-s text-s font-medium text-secondary-text outline-none data-highlighted:bg-control-hover data-highlighted:text-foreground"
              >
                <Icon icon={op === "subtract" ? MinusSignIcon : PlusSignIcon} size={12} />
                {label}
              </Menu.Item>
            ))}
          </Menu.Popup>
        </Menu.Positioner>
      </Menu.Portal>
    </Menu.Root>
  );
}

function ComponentRow({ mask, index }: { mask: Mask; index: number }) {
  const component = mask.components[index];
  if (!component) {
    return null;
  }
  return (
    <li className="flex flex-col gap-xs">
      <div className="flex h-8 items-center gap-xs rounded-m bg-control pr-xxs pl-xs">
        {index > 0 ? (
          <OpMenu mask={mask} index={index} />
        ) : (
          <span className="grid size-6 shrink-0 place-items-center text-s font-heavy text-tertiary-text">
            {opSigns.add}
          </span>
        )}
        <span className="min-w-0 grow truncate text-s font-medium text-foreground">
          {componentName(mask, index)}
        </span>
        <IconAction
          icon={ContrastIcon}
          label={`Invert ${componentName(mask, index)}`}
          pressed={component.invert}
          onClick={() => editComponent(mask.id, index, "invert")}
        />
        <IconAction
          icon={Delete02Icon}
          label={`Delete ${componentName(mask, index)}`}
          onClick={() => editComponent(mask.id, index, "delete")}
        />
      </div>
      <RangeControls maskId={mask.id} index={index} shape={component.shape} />
    </li>
  );
}

export function ComponentList({ mask }: { mask: Mask }) {
  return (
    <div className="flex flex-col gap-s p-xs">
      <ul className="flex flex-col gap-xs" aria-label="Mask components">
        {[...mask.components.keys()].map((position) => (
          <ComponentRow key={`${mask.id}:${position}`} mask={mask} index={position} />
        ))}
      </ul>
      <fieldset className="m-0 flex gap-xs border-0 p-0">
        <legend className="sr-only">Combine with another selection</legend>
        {ops.map(({ op, label }) => (
          <KindMenu key={op} label={label} onPick={(kind) => addComponent(mask.id, op, kind)} />
        ))}
      </fieldset>
    </div>
  );
}
