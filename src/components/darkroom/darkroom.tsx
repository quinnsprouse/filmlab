import { useEffect, useState } from "react";

import { useStore } from "@/hooks/use-store";
import { startRuntime } from "@/lib/engine/runtime";
import { restoreFilmFolder } from "@/lib/film/stocks";
import { library } from "@/lib/library/library";
import { loadProfiles } from "@/lib/profiles/profiles";
import { ui } from "@/lib/ui";

import { Browse } from "./browse/browse";
import { Develop } from "./develop/develop";
import { FirstRun } from "./first-run";
import { TopBar } from "./top-bar";

type Boot = { state: "starting" } | { state: "ready" } | { state: "failed"; message: string };

export function Darkroom() {
  const [boot, setBoot] = useState<Boot>({ state: "starting" });
  const hasFolder = useStore(library, (state) => state.folder !== undefined);
  const mode = useStore(ui, (state) => state.mode);

  useEffect(() => {
    let live = true;
    Promise.all([startRuntime(), loadProfiles(), restoreFilmFolder().catch(() => undefined)]).then(
      () => live && setBoot({ state: "ready" }),
      (error: unknown) => live && setBoot({ state: "failed", message: String(error) }),
    );
    return () => {
      live = false;
    };
  }, []);

  if (boot.state === "failed") {
    return (
      <div className="grid h-dvh place-items-center bg-canvas p-l text-center">
        <p className="max-w-[48ch] text-s text-secondary-text">
          Unable to start the photo engine. {boot.message}
        </p>
      </div>
    );
  }

  return (
    <div className="flex h-dvh flex-col overflow-hidden bg-canvas text-foreground">
      <TopBar />
      <div className="min-h-0 flex-1">
        {!hasFolder ? (
          <FirstRun ready={boot.state === "ready"} />
        ) : mode === "develop" ? (
          <Develop />
        ) : (
          <Browse />
        )}
      </div>
    </div>
  );
}
