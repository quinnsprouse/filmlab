import { develop, openPhoto } from "@/lib/develop/session";
import { library } from "@/lib/library/library";
import { focusPhoto, setMode, ui } from "@/lib/ui";

export function showBrowse(): void {
  setMode("browse");
}

export function showDevelop(): void {
  const { focused, selection } = ui.get();
  const id = focused ?? selection[0] ?? library.get().photos[0]?.file.id;
  if (!id) {
    return;
  }
  if (focused !== id) {
    focusPhoto(id);
  }
  setMode("develop");
  if (develop.get().photoId !== id) {
    void openPhoto(id);
  }
}
