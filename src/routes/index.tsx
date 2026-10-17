import { ClientOnly, createFileRoute } from "@tanstack/react-router";
import { lazy, Suspense } from "react";

const Darkroom = lazy(() =>
  import("@/components/darkroom/darkroom").then((module) => ({ default: module.Darkroom })),
);

export const Route = createFileRoute("/")({
  component: Home,
});

function Splash() {
  return <div className="h-dvh bg-canvas" aria-busy="true" />;
}

function Home() {
  return (
    <ClientOnly fallback={<Splash />}>
      <Suspense fallback={<Splash />}>
        <Darkroom />
      </Suspense>
    </ClientOnly>
  );
}
