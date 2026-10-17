import { useStore } from "@/hooks/use-store";
import { library } from "@/lib/library/library";
import { cn } from "@/lib/utils";

import { photoCount, type DayGroup } from "./model";
import { OpenFolderButton } from "./open-folder-button";

interface RailProps {
  groups: DayGroup[];
  inView: string | undefined;
  onJump: (key: string) => void;
}

function FolderCard() {
  const name = useStore(library, (state) => state.folder?.name);
  const total = useStore(library, (state) => state.photos.length);
  const cover = useStore(
    library,
    (state) => state.photos.find((record) => record.thumbnail)?.thumbnail,
  );
  return (
    <div className="flex h-[52px] items-center gap-s rounded-m bg-control-active pr-2.5 pl-s">
      {cover ? (
        <img
          src={cover}
          alt=""
          className="h-[40px] w-[60px] shrink-0 rounded-xs object-cover outline outline-1 -outline-offset-1 outline-outline-image"
        />
      ) : (
        <div className="h-[40px] w-[60px] shrink-0 rounded-xs bg-control" />
      )}
      <div className="flex min-w-0 flex-col gap-xxs">
        <span className="truncate text-s font-medium text-foreground">{name}</span>
        <span className="truncate font-data text-xs font-medium text-secondary-text">
          {photoCount(total)}
        </span>
      </div>
    </div>
  );
}

function DayRow({
  group,
  current,
  onJump,
}: { group: DayGroup; current: boolean } & Pick<RailProps, "onJump">) {
  return (
    <button
      type="button"
      aria-current={current}
      onClick={() => onJump(group.key)}
      className={cn(
        "relative flex h-8 shrink-0 items-center justify-between rounded-xs px-2.5 text-s font-medium transition-colors duration-(--motion-duration) ease-(--motion-easing) before:absolute before:inset-y-0 before:left-0 before:w-[2px] hover:bg-control",
        current ? "text-foreground before:bg-foreground" : "text-secondary-text before:bg-border",
      )}
    >
      {group.label}
      <span
        className={cn(
          "font-data text-xs tabular-nums",
          current ? "text-foreground" : "text-tertiary-text",
        )}
      >
        {group.tiles.length}
      </span>
    </button>
  );
}

export function Rail({ groups, inView, onJump }: RailProps) {
  return (
    <aside className="relative flex w-[248px] shrink-0 flex-col gap-l bg-chrome px-s py-5 after:absolute after:inset-y-0 after:right-0 after:w-px after:bg-border">
      <div className="flex min-h-0 flex-col gap-xxs">
        <h2 className="flex h-8 shrink-0 items-center px-2.5 pb-xs text-s font-heavy text-secondary-text">
          Folder
        </h2>
        <FolderCard />
        <nav aria-label="Days" className="flex min-h-0 flex-col gap-xxs overflow-y-auto pt-s pl-m">
          {groups.map((group) => (
            <DayRow key={group.key} group={group} current={group.key === inView} onJump={onJump} />
          ))}
        </nav>
      </div>
      <OpenFolderButton className="w-full" />
    </aside>
  );
}
