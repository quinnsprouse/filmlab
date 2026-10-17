# UI and motion

For UI work, follow `GUI.md` and the `graphical-ui` skill. This guide covers how the starter wires components, tokens, fonts, and motion.

- **Check `src/components/ui/` before you build a component.** Add a missing one with `npx shadcn@latest add <component>`. It installs on Base UI in the `base-mira` style.
- **Actions are `Button`, navigation is `<Link>`.** To make a link look like a button, style it with `buttonVariants({ variant, size })`, so assistive technology still announces a link.
- **Icons come from `@/components/icons`** through its `Icon` wrapper. Give an icon-only button an accessible name.
- **Use the theme's names.** `src/styles/app.css` defines the Untitled tokens, each color a `light-dark()` pair that follows the OS theme. Write `text-m`, `p-m`, `gap-s`, `rounded-s`, `font-heavy`, `bg-primary`, and `text-muted-foreground`. Tailwind's default type, radius, shadow, weight, and palette scales are gone, so `text-sm` or `bg-emerald-500` fails lint.
- **Draw edges with `ring` or `inset-ring`**, and dividers with `divider-t` or `divider-b`, not `border`. The theme draws every edge as a box shadow.
- **Fonts come from Fontsource**, imported in `app.css`: Open Runde for text and Geist Mono for code. Never link a third-party font stylesheet.
- **Load motion lazily.** Import from `motion/react`, render `m` components inside `<LazyMotion strict>`, and load the features from `@/lib/motion-features` with `import()`, as `src/components/terminal-demo.tsx` does.
- **Motion renders `initial` on the server.** If `initial` hides content, add `noscript:opacity-100!` so the content shows without JavaScript.
- **Time motion with the theme.** Use `duration-(--motion-duration) ease-(--motion-easing)` for controls, and the large curve for reveals and panels. Animate `transform` and `opacity`, never `transition: all`. Respect reduced motion with `usePrefersReducedMotion`.
- **Keep input text at least `16px`**, or iOS zooms in on focus. Use `tabular-nums` for numbers that change.
