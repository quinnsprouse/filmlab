import { Popover } from "@base-ui/react/popover";
import { Switch } from "@base-ui/react/switch";
import { useRef, useState, type SyntheticEvent } from "react";

import { Add01Icon, Icon } from "@/components/icons";
import { Button } from "@/components/ui/button";
import { lookGroups, perPhotoGroups } from "@/lib/develop/panel";
import { develop } from "@/lib/develop/session";
import { saveProfile } from "@/lib/profiles/profiles";

import { groupLabels } from "./group-labels";

interface GroupListProps {
  title: string;
  hint: string;
  groups: readonly string[];
  chosen: Set<string>;
  onToggle: (group: string, on: boolean) => void;
}

function GroupList({ title, hint, groups, chosen, onToggle }: GroupListProps) {
  return (
    <fieldset className="flex flex-col gap-xs pt-s pb-xs divider-t">
      <legend className="sr-only">{title}</legend>
      <div className="flex items-baseline justify-between pb-xs" aria-hidden>
        <span className="text-s font-heavy text-secondary-text">{title}</span>
        <span className="text-xs text-tertiary-text">{hint}</span>
      </div>
      {groups.map((group) => (
        <label
          key={group}
          className="flex h-9 items-center justify-between rounded-m bg-control px-s text-s font-medium text-foreground"
        >
          {groupLabels[group] ?? group}
          <Switch.Root
            checked={chosen.has(group)}
            onCheckedChange={(on) => onToggle(group, on)}
            className="relative h-5 w-9 shrink-0 rounded-full bg-control-active transition-colors duration-(--motion-duration) ease-(--motion-easing) data-[checked]:bg-border-strong"
          >
            <Switch.Thumb className="block size-4 translate-x-0.5 rounded-full bg-secondary-text transition-transform duration-(--motion-duration) ease-(--motion-easing) data-[checked]:translate-x-[18px] data-[checked]:bg-foreground" />
          </Switch.Root>
        </label>
      ))}
    </fieldset>
  );
}

interface FormProps {
  onDone: () => void;
  inputRef: React.RefObject<HTMLInputElement | null>;
}

function SaveForm({ onDone, inputRef }: FormProps) {
  const [name, setName] = useState("");
  const [chosen, setChosen] = useState(() => new Set<string>(lookGroups));
  const canSave = name.trim().length > 0;

  const toggle = (group: string, on: boolean) =>
    setChosen((current) => {
      const next = new Set(current);
      if (on) {
        next.add(group);
      } else {
        next.delete(group);
      }
      return next;
    });

  const submit = (event: SyntheticEvent) => {
    event.preventDefault();
    const settings = develop.get().settings;
    if (!canSave || !settings) {
      return;
    }
    void saveProfile(name, settings, [...chosen], develop.get().film);
    onDone();
  };

  return (
    <form onSubmit={submit} className="flex flex-col">
      <div className="flex items-center justify-between pb-s divider-b">
        <Popover.Title className="text-m font-heavy text-foreground">Save profile</Popover.Title>
      </div>
      <label className="mt-m flex h-9 items-center gap-m rounded-m bg-control-active px-s inset-ring inset-ring-border-strong">
        <span className="text-s font-medium text-secondary-text">Name</span>
        <input
          ref={inputRef}
          value={name}
          onChange={(event) => setName(event.target.value)}
          placeholder="Golden stone"
          className="min-w-0 grow bg-transparent text-s font-medium text-foreground outline-none placeholder:text-tertiary-text"
        />
      </label>
      <div className="mt-s flex max-h-[50dvh] flex-col overflow-y-auto">
        <GroupList
          title="Look"
          hint="Saved in the profile"
          groups={lookGroups}
          chosen={chosen}
          onToggle={toggle}
        />
        <GroupList
          title="Per photo"
          hint="Left alone when applied"
          groups={perPhotoGroups}
          chosen={chosen}
          onToggle={toggle}
        />
      </div>
      <div className="flex gap-s pt-m divider-t">
        <Button type="button" variant="secondary" className="grow" onClick={onDone}>
          Cancel
        </Button>
        <Button type="submit" className="grow" disabled={!canSave}>
          Save profile
        </Button>
      </div>
    </form>
  );
}

export function SaveProfile() {
  const [open, setOpen] = useState(false);
  const inputRef = useRef<HTMLInputElement>(null);
  return (
    <Popover.Root open={open} onOpenChange={setOpen}>
      <Popover.Trigger render={<Button variant="secondary" className="w-full" />}>
        <Icon icon={Add01Icon} size={15} />
        Save as profile
      </Popover.Trigger>
      <Popover.Portal>
        <Popover.Positioner side="right" align="start" sideOffset={12} className="z-50">
          <Popover.Popup
            initialFocus={inputRef}
            className="w-[360px] rounded-l bg-panel p-m text-foreground shadow-m ring ring-border"
          >
            <SaveForm inputRef={inputRef} onDone={() => setOpen(false)} />
          </Popover.Popup>
        </Popover.Positioner>
      </Popover.Portal>
    </Popover.Root>
  );
}
