import { useStore } from "@/hooks/use-store";
import { develop } from "@/lib/develop/session";
import { library } from "@/lib/library/library";
import { ui } from "@/lib/ui";

export function useExportIds(): string[] {
  const mode = useStore(ui, (state) => state.mode);
  const selection = useStore(ui, (state) => state.selection);
  const focused = useStore(ui, (state) => state.focused);
  const photos = useStore(library, (state) => state.photos);
  const openId = useStore(develop, (state) => state.photoId);
  if (mode === "develop") {
    const id = openId ?? focused;
    return id ? [id] : [];
  }
  return selection.length > 0 ? selection : photos.map((photo) => photo.file.id);
}
