import { CropPanel } from "@/components/darkroom/develop/crop/crop-panel";
import { DevelopPanel } from "@/components/darkroom/develop/develop-panel";
import { HealPanel } from "@/components/darkroom/develop/heal/heal-panel";
import { MasksPanel } from "@/components/darkroom/develop/masks/masks-panel";
import { useStore } from "@/hooks/use-store";
import { develop } from "@/lib/develop/session";

export function ToolPanel() {
  const tool = useStore(develop, (state) => state.tool);
  switch (tool) {
    case "edit":
      return <DevelopPanel />;
    case "crop":
      return <CropPanel />;
    case "heal":
      return <HealPanel />;
    case "masks":
      return <MasksPanel />;
  }
}
