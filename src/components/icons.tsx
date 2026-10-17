import {
  Add01Icon,
  Alert02Icon,
  ArrowDown01Icon,
  BandageIcon,
  Cancel01Icon,
  CheckmarkCircle02Icon,
  CropIcon,
  Delete02Icon,
  Download04Icon,
  Flag02Icon,
  Folder01Icon,
  FolderOpenIcon,
  InformationCircleIcon,
  Loading03Icon,
  Redo02Icon,
  Tick02Icon,
  Undo02Icon,
} from "@hugeicons/core-free-icons";
import { HugeiconsIcon, type HugeiconsIconProps, type IconSvgElement } from "@hugeicons/react";

interface IconProps extends Omit<HugeiconsIconProps, "icon" | "strokeWidth"> {
  icon: IconSvgElement;
  className?: string;
}

export function Icon({ icon, className, size = 16, ...props }: IconProps) {
  return (
    <HugeiconsIcon icon={icon} size={size} className={className} data-slot="icon" {...props} />
  );
}

export {
  Add01Icon,
  Alert02Icon,
  ArrowDown01Icon,
  BandageIcon,
  Cancel01Icon,
  CheckmarkCircle02Icon,
  CropIcon,
  Delete02Icon,
  Download04Icon,
  Flag02Icon,
  Folder01Icon,
  FolderOpenIcon,
  InformationCircleIcon,
  Loading03Icon,
  Redo02Icon,
  Tick02Icon,
  Undo02Icon,
};
