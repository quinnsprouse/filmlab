import { CropOverlay } from "@/components/darkroom/develop/crop/crop-overlay";
import { HealOverlay } from "@/components/darkroom/develop/heal/heal-overlay";
import { MasksOverlay } from "@/components/darkroom/develop/masks/masks-overlay";
import { useStore } from "@/hooks/use-store";
import { develop } from "@/lib/develop/session";

export function ToolOverlay() {
  const tool = useStore(develop, (state) => state.tool);
  switch (tool) {
    case "edit":
      return null;
    case "crop":
      return <CropOverlay />;
    case "heal":
      return <HealOverlay />;
    case "masks":
      return <MasksOverlay />;
  }
}
