import { Icon, Redo02Icon, Undo02Icon } from "@/components/icons";
import { Button } from "@/components/ui/button";
import { useStore } from "@/hooks/use-store";
import { develop, redo, setCompare, undo } from "@/lib/develop/session";

import { Segmented } from "./segmented";

export function DevelopActions() {
  const compare = useStore(develop, (state) => state.compare);
  const cursor = useStore(develop, (state) => state.cursor);
  const steps = useStore(develop, (state) => state.history.length);
  return (
    <>
      <Segmented
        label="Compare"
        value={compare}
        options={[
          { value: "before", label: "Before" },
          { value: "after", label: "After" },
        ]}
        onChange={setCompare}
      />
      <div className="flex">
        <Button variant="ghost" size="icon" aria-label="Undo" disabled={cursor <= 0} onClick={undo}>
          <Icon icon={Undo02Icon} />
        </Button>
        <Button
          variant="ghost"
          size="icon"
          aria-label="Redo"
          disabled={cursor >= steps - 1}
          onClick={redo}
        >
          <Icon icon={Redo02Icon} />
        </Button>
      </div>
    </>
  );
}
