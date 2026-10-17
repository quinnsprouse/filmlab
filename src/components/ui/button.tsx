import { Button as ButtonPrimitive } from "@base-ui/react/button";
import { cva, type VariantProps } from "class-variance-authority";

import { cn } from "@/lib/utils";

const buttonVariants = cva(
  "group/button inline-flex shrink-0 items-center justify-center gap-xs rounded-s font-ui text-m font-medium whitespace-nowrap transition-[background-color,box-shadow,color,translate] duration-(--motion-duration) ease-(--motion-easing) select-none focus-visible:inset-ring-primary-transparent active:not-aria-[haspopup]:translate-y-(--motion-press-distance) disabled:pointer-events-none disabled:text-neutral-6 [&_svg]:pointer-events-none [&_svg]:shrink-0 [&_svg:not([class*='size-'])]:size-4",
  {
    variants: {
      variant: {
        default:
          "bg-primary text-primary-foreground inset-ring inset-ring-primary-transparent hover:shadow-s disabled:bg-muted disabled:inset-ring-neutral-4-transparent",
        secondary:
          "bg-secondary text-secondary-foreground ring ring-neutral-3-transparent hover:bg-neutral-4-transparent hover:ring-neutral-4-transparent",
        outline:
          "text-foreground inset-ring inset-ring-input hover:bg-neutral-3-transparent disabled:inset-ring-neutral-4-transparent",
        ghost: "text-foreground hover:bg-neutral-3-transparent",
        destructive:
          "bg-destructive text-destructive-foreground inset-ring inset-ring-destructive-transparent hover:shadow-s disabled:bg-muted disabled:inset-ring-neutral-4-transparent",
        link: "text-primary underline-offset-4 hover:underline",
      },
      size: {
        default: "px-m py-xs",
        icon: "p-xs",
      },
    },
    defaultVariants: {
      variant: "default",
      size: "default",
    },
  },
);

function Button({
  className,
  variant = "default",
  size = "default",
  ...props
}: ButtonPrimitive.Props & VariantProps<typeof buttonVariants>) {
  return (
    <ButtonPrimitive
      data-slot="button"
      className={cn(buttonVariants({ variant, size, className }))}
      {...props}
    />
  );
}

export { Button, buttonVariants };
