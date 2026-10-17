import { useEffect, useRef } from "react";

import { Flag02Icon, Icon } from "@/components/icons";
import { useStore } from "@/hooks/use-store";
import { develop, openPhoto } from "@/lib/develop/session";
import { library, type PhotoRecord } from "@/lib/library/library";
import { focusPhoto } from "@/lib/ui";
import { cn } from "@/lib/utils";

const frameHeight = 64;

function Sprockets() {
  return (
    <svg aria-hidden className="h-3 w-full shrink-0 text-foreground/10">
      <defs>
        <pattern id="sprockets" width="15" height="12" patternUnits="userSpaceOnUse">
          <rect width="7" height="12" fill="currentColor" />
        </pattern>
      </defs>
      <rect width="100%" height="12" fill="url(#sprockets)" />
    </svg>
  );
}

export function goToPhoto(id: string): void {
  focusPhoto(id);
  void openPhoto(id);
}

function isTyping(target: EventTarget | null): boolean {
  return (
    target instanceof HTMLElement &&
    (target.isContentEditable ||
      ["INPUT", "TEXTAREA", "SELECT"].includes(target.tagName) ||
      target.getAttribute("role") === "slider")
  );
}

function stepPhoto(offset: number): void {
  const { photos } = library.get();
  const current = photos.findIndex((record) => record.file.id === develop.get().photoId);
  const next = photos[current + offset];
  if (current >= 0 && next) {
    goToPhoto(next.file.id);
  }
}

function frameWidth(record: PhotoRecord): number {
  const facts = record.probed?.facts;
  return facts ? Math.round((frameHeight * facts.width) / facts.height) : (frameHeight * 3) / 2;
}

function frameNumber(record: PhotoRecord): string {
  return record.file.stem.match(/\d{4}$/)?.[0] ?? "";
}

interface FrameProps {
  record: PhotoRecord;
  index: number;
  current: boolean;
}

function Frame({ record, index, current }: FrameProps) {
  const ref = useRef<HTMLButtonElement>(null);
  const flag = record.state?.flag ?? "none";

  useEffect(() => {
    if (current) {
      ref.current?.scrollIntoView({ inline: "center", block: "nearest" });
    }
  }, [current]);

  return (
    <button
      ref={ref}
      type="button"
      aria-label={record.file.name}
      aria-current={current}
      onClick={() => goToPhoto(record.file.id)}
      className="flex shrink-0 cursor-default flex-col gap-s text-left"
    >
      <span
        className={cn(
          "relative block h-16 w-(--frame-width) bg-control bg-(image:--thumb) bg-cover bg-center",
          current
            ? "outline outline-2 outline-offset-2 outline-primary"
            : "outline outline-1 -outline-offset-1 outline-outline-image",
        )}
        style={{
          "--frame-width": `${frameWidth(record)}px`,
          "--thumb": record.thumbnail ? `url(${record.thumbnail})` : "none",
        }}
      >
        {flag === "none" ? null : (
          <Icon
            icon={Flag02Icon}
            size={14}
            className={cn(
              "absolute top-xxs right-xxs",
              flag === "pick" ? "text-foreground" : "text-secondary-text opacity-70",
            )}
          />
        )}
      </span>
      <span
        className={cn(
          "font-data text-xxs leading-3.5 whitespace-pre tabular-nums",
          current ? "font-heavy text-primary" : "text-tertiary-text",
        )}
      >
        {`▸${index + 1}${current ? `  ${frameNumber(record)}` : ""}`}
      </span>
    </button>
  );
}

export function Filmstrip() {
  const photos = useStore(library, (state) => state.photos);
  const photoId = useStore(develop, (state) => state.photoId);

  useEffect(() => {
    const onKey = (event: KeyboardEvent) => {
      if (isTyping(event.target) || event.metaKey || event.ctrlKey || event.altKey) {
        return;
      }
      if (event.key === "ArrowLeft" || event.key === "ArrowRight") {
        event.preventDefault();
        stepPhoto(event.key === "ArrowLeft" ? -1 : 1);
      }
    };
    window.addEventListener("keydown", onKey);
    return () => window.removeEventListener("keydown", onKey);
  }, []);

  return (
    <div className="flex h-[140px] shrink-0 flex-col justify-between overflow-hidden bg-chrome py-xs divider-t">
      <Sprockets />
      <div className="flex [scrollbar-width:none] items-start gap-s overflow-x-auto px-m py-xxs">
        {photos.map((record, index) => (
          <Frame
            key={record.file.id}
            record={record}
            index={index}
            current={record.file.id === photoId}
          />
        ))}
      </div>
      <Sprockets />
    </div>
  );
}
