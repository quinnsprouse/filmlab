import { Link } from "@tanstack/react-router";

import { buttonVariants } from "@/components/ui/button";

export function RouteNotFoundComponent() {
  return (
    <div className="flex min-h-dvh flex-col items-center justify-center gap-l px-m text-center">
      <div className="space-y-xs">
        <p className="text-s font-medium text-muted-foreground">404</p>
        <h1 className="font-brand text-xl font-heavy text-balance">Page not found</h1>
        <p className="text-muted-foreground">
          The page you are looking for either moved or does not exist.
        </p>
      </div>
      <Link to="/" search={{}} className={buttonVariants()}>
        Back home
      </Link>
    </div>
  );
}
