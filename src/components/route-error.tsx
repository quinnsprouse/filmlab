import { Link, useRouter } from "@tanstack/react-router";
import type { ErrorComponentProps } from "@tanstack/react-router";

import { Button, buttonVariants } from "@/components/ui/button";

const productionMessage = "The application hit an unexpected error. You can retry or return home.";

export function RouteErrorComponent({ error }: ErrorComponentProps) {
  const router = useRouter();
  const message = import.meta.env.DEV && error instanceof Error ? error.message : productionMessage;

  return (
    <div className="flex min-h-dvh flex-col items-center justify-center gap-l px-m text-center">
      <div className="space-y-xs">
        <p className="text-s font-medium text-muted-foreground">Error</p>
        <h1 className="font-brand text-xl font-heavy text-balance">Something went wrong</h1>
        <p className="text-muted-foreground">{message}</p>
      </div>
      <div className="flex flex-wrap justify-center gap-s">
        <Button type="button" onClick={() => void router.invalidate()}>
          Try again
        </Button>
        <Link to="/" search={{}} className={buttonVariants({ variant: "outline" })}>
          Back home
        </Link>
      </div>
    </div>
  );
}
