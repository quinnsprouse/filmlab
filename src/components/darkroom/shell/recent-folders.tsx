import { useEffect, useState } from "react";

import { Folder01Icon, Icon } from "@/components/icons";
import { Button } from "@/components/ui/button";
import { recentFolders, type RecentFolder } from "@/lib/folder/recent";

function openedAgo(iso: string): string {
  const days = Math.floor((Date.now() - new Date(iso).getTime()) / 86_400_000);
  if (days <= 0) {
    return "today";
  }
  if (days === 1) {
    return "yesterday";
  }
  if (days < 7) {
    return `${days} days ago`;
  }
  return new Date(iso).toLocaleDateString(undefined, { month: "short", day: "numeric" });
}

interface RecentFoldersProps {
  onReopen: (entry: RecentFolder) => void;
}

export function RecentFolders({ onReopen }: RecentFoldersProps) {
  const [entries, setEntries] = useState<RecentFolder[]>([]);

  useEffect(() => {
    let live = true;
    recentFolders().then(
      (found) => live && setEntries(found),
      () => undefined,
    );
    return () => {
      live = false;
    };
  }, []);

  if (entries.length === 0) {
    return null;
  }
  return (
    <section aria-label="Recent folders" className="flex flex-col gap-xxs">
      <h2 className="px-2.5 pb-xs text-s font-heavy text-secondary-text">Recent</h2>
      {entries.map((entry) => (
        <div
          key={`${entry.name}-${entry.openedAt}`}
          className="flex h-14 items-center gap-s rounded-m bg-control pr-xs pl-s"
        >
          <span className="grid h-11 w-[66px] shrink-0 place-items-center rounded-xs bg-control-active text-tertiary-text">
            <Icon icon={Folder01Icon} size={20} />
          </span>
          <div className="flex min-w-0 grow flex-col gap-xxs">
            <span className="truncate text-s font-medium text-foreground">{entry.name}</span>
            <span className="truncate font-data text-xs font-medium text-secondary-text">
              {entry.photos} photos · opened {openedAgo(entry.openedAt)}
            </span>
          </div>
          <Button variant="secondary" onClick={() => onReopen(entry)}>
            Reopen
          </Button>
        </div>
      ))}
    </section>
  );
}
