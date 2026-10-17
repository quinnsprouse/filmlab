import { Menu } from "@base-ui/react/menu";

import { createKinds, KindGlyph } from "./create-grid";
import { kindLabels, type CreateKind } from "./masks-model";

interface KindMenuProps {
  label: string;
  onPick: (kind: CreateKind) => void;
}

export function KindMenu({ label, onPick }: KindMenuProps) {
  return (
    <Menu.Root>
      <Menu.Trigger className="flex h-8 grow basis-0 items-center justify-center rounded-m bg-control text-s font-medium text-secondary-text transition-colors duration-(--motion-duration) ease-(--motion-easing) hover:bg-control-hover hover:text-foreground data-popup-open:bg-control-hover data-popup-open:text-foreground">
        {label}
      </Menu.Trigger>
      <Menu.Portal>
        <Menu.Positioner sideOffset={4} align="start">
          <Menu.Popup className="min-w-[176px] rounded-m bg-panel p-xxs ring ring-border-strong">
            {createKinds.map((kind) => (
              <Menu.Item
                key={kind}
                onClick={() => onPick(kind)}
                className="flex h-8 cursor-default items-center gap-s rounded-s px-s text-s font-medium text-secondary-text outline-none data-highlighted:bg-control-hover data-highlighted:text-foreground"
              >
                <KindGlyph kind={kind} size={14} />
                {kindLabels[kind]}
              </Menu.Item>
            ))}
          </Menu.Popup>
        </Menu.Positioner>
      </Menu.Portal>
    </Menu.Root>
  );
}
