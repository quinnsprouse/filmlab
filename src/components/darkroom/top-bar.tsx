import { Download04Icon, Icon } from "@/components/icons";
import { Button } from "@/components/ui/button";
import { useStore } from "@/hooks/use-store";
import { develop } from "@/lib/develop/session";
import { library } from "@/lib/library/library";
import { ui } from "@/lib/ui";

import { DevelopActions } from "./shell/develop-actions";
import { ExportDialog } from "./shell/export-dialog";
import { openExport } from "./shell/export-state";
import { showBrowse, showDevelop } from "./shell/navigation";
import { SaveStatus } from "./shell/save-status";
import { Segmented } from "./shell/segmented";
import { useExportIds } from "./shell/use-export-ids";
import { useShortcuts } from "./shell/use-shortcuts";

function FolderPath({ name }: { name: string }) {
  return (
    <span className="font-data text-xs font-medium text-secondary-text">
      {name.split("/").join(" / ")}
    </span>
  );
}

function Position() {
  const photoId = useStore(develop, (state) => state.photoId);
  const photos = useStore(library, (state) => state.photos);
  const index = photos.findIndex((photo) => photo.file.id === photoId);
  if (index < 0) {
    return null;
  }
  return (
    <span className="font-data text-xs font-medium text-tertiary-text tabular-nums">
      {index + 1} / {photos.length}
    </span>
  );
}

export function TopBar() {
  useShortcuts();
  const folderName = useStore(library, (state) => state.folder?.name);
  const photoCount = useStore(library, (state) => state.photos.length);
  const mode = useStore(ui, (state) => state.mode);
  const ids = useExportIds();
  const hasFolder = folderName !== undefined;
  const developing = mode === "develop";

  return (
    <header className="flex h-12 shrink-0 items-center justify-between bg-chrome px-m divider-b">
      <div className="flex w-[420px] shrink-0 items-center gap-m">
        <span className="text-m font-heavy">filmlab</span>
        {hasFolder && (
          <>
            <Segmented
              label="Mode"
              value={mode}
              options={[
                { value: "browse", label: "Browse" },
                { value: "develop", label: "Develop", disabled: photoCount === 0 },
              ]}
              onChange={(next) => (next === "browse" ? showBrowse() : showDevelop())}
            />
            {developing && <Position />}
          </>
        )}
      </div>
      {hasFolder && (
        <div className="flex min-w-0 items-center gap-s">
          <FolderPath name={folderName} />
          <SaveStatus />
        </div>
      )}
      <div className="flex w-[420px] shrink-0 items-center justify-end gap-s">
        {hasFolder && (
          <>
            {developing && <DevelopActions />}
            <Button disabled={ids.length === 0} onClick={openExport}>
              <Icon icon={Download04Icon} />
              {developing ? "Export" : `Export ${ids.length}`}
            </Button>
            <ExportDialog />
          </>
        )}
      </div>
    </header>
  );
}
