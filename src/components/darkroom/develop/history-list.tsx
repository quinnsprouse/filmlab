import { useStore } from "@/hooks/use-store";
import { develop, goToStep } from "@/lib/develop/session";
import { cn } from "@/lib/utils";

export function HistoryList() {
  const history = useStore(develop, (state) => state.history);
  const cursor = useStore(develop, (state) => state.cursor);
  const steps = history.map((step, index) => ({ step, index })).toReversed();

  return (
    <section className="flex min-h-0 flex-col gap-xxs" aria-label="History">
      <div className="flex shrink-0 items-center justify-between px-s pb-xs">
        <h2 className="text-s font-heavy text-secondary-text">History</h2>
        <span className="font-data text-xs text-tertiary-text tabular-nums">
          {history.length} {history.length === 1 ? "step" : "steps"}
        </span>
      </div>
      <ul className="flex min-h-0 flex-col gap-xxs overflow-y-auto">
        {steps.map(({ step, index }) => (
          <li key={`${index}-${step.key}`} className="shrink-0">
            <button
              type="button"
              aria-current={index === cursor}
              onClick={() => goToStep(index)}
              className={cn(
                "flex h-8 w-full items-center rounded-m px-s text-left text-s font-medium transition-colors duration-(--motion-duration) ease-(--motion-easing) hover:bg-control",
                index === cursor && "bg-control-active text-foreground",
                index < cursor && "text-secondary-text",
                index > cursor && "text-tertiary-text opacity-60",
              )}
            >
              <span className="truncate">{step.label}</span>
            </button>
          </li>
        ))}
      </ul>
    </section>
  );
}
