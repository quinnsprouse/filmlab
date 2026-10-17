import { useEffect } from "react";

import { useStore } from "@/hooks/use-store";
import { develop, flushEdits } from "@/lib/develop/session";
import { library } from "@/lib/library/library";
import { ui } from "@/lib/ui";

import { Filmstrip, goToPhoto } from "./filmstrip";
import { Histogram } from "./histogram";
import { HistoryList } from "./history-list";
import { ProfilesSection } from "./profiles-section";
import { Stage } from "./stage";
import { ToolsRow } from "./tools-row";
import { ToolPanel } from "./tools/tool-panel";

function targetId(): string | undefined {
  return ui.get().focused ?? library.get().photos[0]?.file.id;
}

function openFocusedPhoto() {
  const target = targetId();
  if (target && develop.get().photoId !== target) {
    goToPhoto(target);
  }
}

export function Develop() {
  const ready = useStore(
    library,
    (state) => state.photos.find((record) => record.file.id === targetId())?.probed !== undefined,
  );

  useEffect(() => {
    if (ready) {
      openFocusedPhoto();
    }
  }, [ready]);

  useEffect(
    () => () => {
      void flushEdits();
    },
    [],
  );

  return (
    <div className="flex h-full min-h-0">
      <aside className="flex w-[248px] shrink-0 flex-col gap-l overflow-hidden bg-chrome px-xs py-m">
        <div className="shrink-0">
          <ProfilesSection />
        </div>
        <div className="flex min-h-0 flex-1 flex-col justify-end">
          <HistoryList />
        </div>
      </aside>
      <div className="flex min-w-0 grow flex-col">
        <Stage />
        <Filmstrip />
      </div>
      <aside className="flex w-[320px] shrink-0 flex-col gap-m overflow-hidden bg-chrome p-m">
        <Histogram />
        <ToolsRow />
        <div className="min-h-0 grow overflow-y-auto rounded-l">
          <ToolPanel />
        </div>
      </aside>
    </div>
  );
}
