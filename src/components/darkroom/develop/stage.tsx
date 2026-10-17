import { useEffect, useRef } from "react";

import { Icon, Loading03Icon } from "@/components/icons";
import { useStore } from "@/hooks/use-store";
import { develop, setViewport } from "@/lib/develop/session";
import { photoRecord } from "@/lib/library/library";
import { cn } from "@/lib/utils";

import { exposureSummary } from "./photo-facts";
import { ToolOverlay } from "./tools/tool-overlay";
import { useZoomPointer } from "./use-zoom-pointer";

function Canvas() {
  const frame = useStore(develop, (state) => state.frame);
  const zoomed = useStore(develop, (state) => state.zoom.level !== "fit");
  const editing = useStore(develop, (state) => state.tool === "edit");
  const ref = useRef<HTMLCanvasElement>(null);
  const pointer = useZoomPointer();

  useEffect(() => {
    const canvas = ref.current;
    if (!canvas || !frame) {
      return;
    }
    canvas.width = frame.pixelWidth;
    canvas.height = frame.pixelHeight;
    canvas
      .getContext("2d")
      ?.putImageData(new ImageData(frame.rgba, frame.pixelWidth, frame.pixelHeight), 0, 0);
  }, [frame]);

  if (!frame) {
    return null;
  }
  const scale = 1 / window.devicePixelRatio;
  return (
    <canvas
      ref={ref}
      aria-label="Photo preview"
      {...(editing ? pointer : {})}
      className={cn(
        "h-(--canvas-height) w-(--canvas-width) touch-none outline outline-1 -outline-offset-1 outline-outline-image",
        editing && (zoomed ? "cursor-grab active:cursor-grabbing" : "cursor-zoom-in"),
      )}
      style={{
        "--canvas-width": `${frame.width * scale}px`,
        "--canvas-height": `${frame.height * scale}px`,
      }}
    />
  );
}

function Placeholder() {
  const photoId = useStore(develop, (state) => state.photoId);
  const hasFrame = useStore(develop, (state) => state.frame !== undefined);
  const thumbnail = photoId ? photoRecord(photoId)?.thumbnail : undefined;
  if (hasFrame || !thumbnail) {
    return null;
  }
  return (
    <img
      src={thumbnail}
      alt=""
      className="absolute inset-0 m-auto max-h-full max-w-full object-contain outline outline-1 -outline-offset-1 outline-outline-image"
    />
  );
}

function Notice({ children }: { children: React.ReactNode }) {
  return (
    <p className="absolute inset-0 grid place-items-center text-s text-secondary-text">
      <span className="flex max-w-[48ch] items-center gap-xs text-center">{children}</span>
    </p>
  );
}

function Caption() {
  const photoId = useStore(develop, (state) => state.photoId);
  const facts = useStore(develop, (state) => state.facts);
  const frame = useStore(develop, (state) => state.frame);
  if (!photoId || !facts || !frame) {
    return null;
  }
  const name = photoRecord(photoId)?.file.name ?? photoId;
  return (
    <div
      className="flex w-(--canvas-width) items-center justify-between gap-m font-data text-xs tabular-nums"
      style={{ "--canvas-width": `${frame.width / window.devicePixelRatio}px` }}
    >
      <span className="truncate text-secondary-text">{name}</span>
      <span className="shrink-0 whitespace-pre text-tertiary-text">{exposureSummary(facts)}</span>
    </div>
  );
}

export function Stage() {
  const areaRef = useRef<HTMLDivElement>(null);
  const status = useStore(develop, (state) => state.status);
  const error = useStore(develop, (state) => state.error);
  const photoId = useStore(develop, (state) => state.photoId);
  const compare = useStore(develop, (state) => state.compare);
  const hasFrame = useStore(develop, (state) => state.frame !== undefined);

  useEffect(() => {
    const area = areaRef.current;
    if (!area) {
      return;
    }
    const observer = new ResizeObserver(([entry]) => {
      const { width, height } = entry?.contentRect ?? area.getBoundingClientRect();
      setViewport(width * window.devicePixelRatio, height * window.devicePixelRatio);
    });
    observer.observe(area);
    return () => observer.disconnect();
  }, []);

  return (
    <div className="flex min-h-0 grow flex-col items-center gap-s px-xl pt-l pb-m">
      <div ref={areaRef} className="relative flex min-h-0 w-full grow items-center justify-center">
        <Placeholder />
        <div className="relative">
          <Canvas />
          {hasFrame ? <ToolOverlay /> : null}
          {compare === "before" && hasFrame ? (
            <span className="absolute top-xs left-xs rounded-xs bg-canvas/70 px-xs py-xxs font-data text-xxs text-foreground">
              Before
            </span>
          ) : null}
        </div>
        {status === "loading" ? (
          <Notice>
            <Icon icon={Loading03Icon} size={16} className="animate-spin" />
            Opening {photoId}…
          </Notice>
        ) : null}
        {status === "failed" ? (
          <Notice>
            Unable to open {photoId}. {error} Pick another photo, or reopen the folder.
          </Notice>
        ) : null}
      </div>
      <Caption />
    </div>
  );
}
