import { useEffect, useLayoutEffect, useRef } from "react";

import { Icon, Loading03Icon } from "@/components/icons";
import { useStore } from "@/hooks/use-store";
import { fitScale, rectOf, type Rect } from "@/lib/develop/camera";
import { develop, setViewport, viewer } from "@/lib/develop/session";
import type { DisplayFrame } from "@/lib/develop/state";
import { photoRecord } from "@/lib/library/library";
import { cn } from "@/lib/utils";

import { exposureSummary } from "./photo-facts";
import { ToolOverlay } from "./tools/tool-overlay";
import { useCamera } from "./use-camera";

const exact = 0.003;

interface Placement {
  width: string;
  height: string;
  transform: string;
}

function placement(frame: DisplayFrame, rect: Rect): Placement {
  const ratio = window.devicePixelRatio;
  let sx = rect.width / frame.width;
  let sy = rect.height / frame.height;
  let { x, y } = rect;
  if (Math.abs(sx - 1) < exact && Math.abs(sy - 1) < exact) {
    sx = 1;
    sy = 1;
    x = Math.round(x);
    y = Math.round(y);
  }
  return {
    width: `${frame.width / ratio}px`,
    height: `${frame.height / ratio}px`,
    transform: `translate(${x / ratio}px, ${y / ratio}px) scale(${sx}, ${sy})`,
  };
}

function FrameCanvas({
  frame,
  rect,
  label,
  className,
}: {
  frame: DisplayFrame;
  rect: Rect;
  label?: string;
  className?: string;
}) {
  const ref = useRef<HTMLCanvasElement>(null);
  const place = placement(frame, rect);

  useLayoutEffect(() => {
    const canvas = ref.current;
    if (!canvas || frame.bitmap.width === 0) {
      return;
    }
    if (canvas.width !== frame.width || canvas.height !== frame.height) {
      canvas.width = frame.width;
      canvas.height = frame.height;
    }
    canvas.getContext("2d", { alpha: false })?.drawImage(frame.bitmap, 0, 0);
  }, [frame]);

  return (
    <canvas
      ref={ref}
      aria-label={label}
      aria-hidden={label ? undefined : true}
      className={cn(
        "absolute top-0 left-0 h-(--frame-height) w-(--frame-width) origin-top-left [transform:var(--frame-place)]",
        className,
      )}
      style={{
        "--frame-width": place.width,
        "--frame-height": place.height,
        "--frame-place": place.transform,
      }}
    />
  );
}

function Photo() {
  const frame = useStore(develop, (state) => state.frame);
  const detail = useStore(develop, (state) => state.detail);
  const hasTool = useStore(develop, (state) => state.tool !== "edit");
  const compare = useStore(develop, (state) => state.compare);
  const { camera, native, viewport } = useStore(viewer, (state) => state);
  if (!frame) {
    return null;
  }
  const zoomed = camera.scale > fitScale(native, viewport) * 1.001;
  const fitRect = rectOf(camera, frame.region, native, viewport);
  const tools = placement(frame, { ...fitRect, width: frame.width, height: frame.height });
  return (
    <>
      <FrameCanvas
        frame={frame}
        rect={fitRect}
        label="Photo preview"
        className="outline outline-1 -outline-offset-1 outline-outline-image"
      />
      {detail ? (
        <FrameCanvas
          frame={detail}
          rect={rectOf(camera, detail.region, native, viewport)}
          className={cn(!zoomed && "invisible", camera.scale > 1 && "[image-rendering:pixelated]")}
        />
      ) : null}
      {hasTool || compare === "before" ? (
        <div
          className="absolute top-0 left-0 h-(--frame-height) w-(--frame-width) [transform:var(--frame-place)]"
          style={{
            "--frame-width": tools.width,
            "--frame-height": tools.height,
            "--frame-place": tools.transform,
          }}
        >
          {hasTool ? <ToolOverlay /> : null}
          {compare === "before" ? (
            <span className="absolute top-xs left-xs rounded-xs bg-canvas/70 px-xs py-xxs font-data text-xxs text-foreground">
              Before
            </span>
          ) : null}
        </div>
      ) : null}
    </>
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
  const zoom = useStore(viewer, (state) =>
    state.camera.scale > fitScale(state.native, state.viewport) * 1.001
      ? Math.round(state.camera.scale * 100)
      : undefined,
  );
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
      <span className="shrink-0 whitespace-pre text-tertiary-text">
        {zoom === undefined ? exposureSummary(facts) : `${zoom}%`}
      </span>
    </div>
  );
}

export function Stage() {
  const areaRef = useRef<HTMLDivElement>(null);
  const status = useStore(develop, (state) => state.status);
  const error = useStore(develop, (state) => state.error);
  const photoId = useStore(develop, (state) => state.photoId);
  const editing = useStore(develop, (state) => state.tool === "edit");
  const zoomed = useStore(
    viewer,
    (state) => state.camera.scale > fitScale(state.native, state.viewport) * 1.001,
  );
  const camera = useCamera(areaRef, editing);

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
      <div
        ref={areaRef}
        {...(editing ? camera : {})}
        className={cn(
          "relative min-h-0 w-full grow touch-none overflow-hidden",
          editing && (zoomed ? "cursor-grab active:cursor-grabbing" : "cursor-zoom-in"),
        )}
      >
        <Placeholder />
        <Photo />
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
