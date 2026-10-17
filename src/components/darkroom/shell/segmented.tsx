import { useId } from "react";

import { cn } from "@/lib/utils";

interface SegmentOption<T extends string> {
  value: T;
  label: string;
  disabled?: boolean;
}

interface SegmentedProps<T extends string> {
  label: string;
  value: T;
  options: SegmentOption<T>[];
  onChange: (value: T) => void;
  className?: string;
}

export function Segmented<T extends string>({
  label,
  value,
  options,
  onChange,
  className,
}: SegmentedProps<T>) {
  const name = useId();
  return (
    <fieldset className={cn("flex rounded-m bg-control p-xxs", className)}>
      <legend className="sr-only">{label}</legend>
      {options.map((option) => (
        <label
          key={option.value}
          className={cn(
            "flex h-7 items-center rounded-s px-s text-s font-medium transition-colors duration-(--motion-duration) ease-(--motion-easing) has-focus-visible:inset-ring has-focus-visible:inset-ring-primary-transparent",
            option.disabled ? "text-tertiary-text" : "cursor-pointer",
            option.value === value
              ? "bg-control-active text-foreground"
              : "text-secondary-text hover:text-foreground",
          )}
        >
          <input
            type="radio"
            className="sr-only"
            name={name}
            value={option.value}
            checked={option.value === value}
            disabled={option.disabled}
            onChange={() => onChange(option.value)}
          />
          {option.label}
        </label>
      ))}
    </fieldset>
  );
}
