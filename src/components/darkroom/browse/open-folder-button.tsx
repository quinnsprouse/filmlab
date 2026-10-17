import { useRef, type ChangeEvent } from "react";

import { FolderOpenIcon, Icon } from "@/components/icons";
import { Button } from "@/components/ui/button";
import { supportsFolderWrites } from "@/lib/folder/folder";
import { openPickedFiles, pickFolder } from "@/lib/folder/pick";

function openFromInput(event: ChangeEvent<HTMLInputElement>) {
  if (event.target.files && event.target.files.length > 0) {
    openPickedFiles(event.target.files);
  }
  event.target.value = "";
}

export function OpenFolderButton({ className }: { className?: string }) {
  const input = useRef<HTMLInputElement>(null);

  async function open() {
    const opened = await pickFolder();
    if (!opened && !supportsFolderWrites()) {
      input.current?.click();
    }
  }

  return (
    <>
      <Button variant="secondary" className={className} onClick={() => void open()}>
        <Icon icon={FolderOpenIcon} />
        Open another folder
      </Button>
      <input
        ref={(element) => {
          input.current = element;
          element?.setAttribute("webkitdirectory", "");
        }}
        type="file"
        multiple
        hidden
        onChange={openFromInput}
      />
    </>
  );
}
