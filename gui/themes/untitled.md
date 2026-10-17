# Untitled

Generated from the current project, including edits awaiting autosave. Return to the [theme index](../themes.md). Install the selected fonts and icons using the setup below; assets are acquired separately from their official sources.

## Font and icon setup

Follow [asset installation](../assets.md) when implementing this theme. Download and configure the selected free assets from their official sources; no font files or icon artwork are bundled. Weights below are the authored requests: load matching static faces or a variable range and report any unavailable weight. Register the exact family aliases below, or map the role tokens to the loader’s actual family.

| Selected font | Roles | Weights | Family stack | Setup |
| --- | --- | --- | --- | --- |
| <code>GT Standard M</code> (paid) | brand, ui, editorial | 400, 500, 700 | <code>"GT Standard M", -apple-system, BlinkMacSystemFont, sans-serif</code> | Reuse supplied licensed webfonts. If missing, ask the user to share their files or identify their location, naming the free fallback below that will be used if unavailable. Honor an existing answer; use that fallback when the user has no files or chooses it. Follow [font guidance](../assets.md) and explain how to add [GT Standard M](https://www.grillitype.com/shops/gt-standard) later. |
| <code>Geist Mono</code> (free) | data | 400, 500, 700 | <code>"Sidebar Geist Mono", monospace</code> | Download and load [Geist Mono](https://vercel.com/font) in the app. |

### Free fallbacks for unavailable paid fonts

Ask for missing licensed webfont files or their location before choosing these fallbacks, unless the user has already said the files are unavailable or chosen a fallback. Explain the named fallback for each affected role and continue independent work while an answer is pending. Install these only for the listed paid selections. Keep the paid family first, insert the installed free family before generic/system fallbacks, and preserve the authored weights. Leave other selected free families in place. Tell the user which fallback is active, link the vendor above, and explain where to add licensed webfont files and their font declarations to restore the selection.

| Paid selection | Roles | Free fallback | Weights | Fallback family stack | Setup |
| --- | --- | --- | --- | --- | --- |
| <code>GT Standard M</code> | brand, ui, editorial | <code>Geist</code> | 400, 500, 700 | <code>"Sidebar Geist", sans-serif</code> | Download and load [Geist](https://vercel.com/font) in the app. |

### Icons

Selected: [Central](https://iconists.co/central), outlined. Use the user’s licensed Central artwork if supplied under the [vendor license](https://iconists.co/license). Otherwise install [Lucide](https://lucide.dev/guide/react/getting-started) (React: <code>lucide-react</code>) and wire it into the app’s icon adapter. Tell the user that Lucide is active and that they can license Central at the vendor link, add its files to the app’s icon asset location, and update that adapter to restore Central.

Preserve icon size, color, weight, style, and accessible names. Match the selected style only where the pack supports it; Lucide’s official pack supplies outlines, so report a filled-style gap rather than claiming exact filled artwork. Keep existing package versions and add only missing packages using the receiving project’s package manager.

## Foundations

```json
{
  "name": "Untitled",
  "text": {
    "l": {
      "size": 24,
      "lineHeight": 32
    },
    "m": {
      "size": 16,
      "lineHeight": 24
    },
    "s": {
      "size": 14,
      "lineHeight": 20
    },
    "xl": {
      "size": 36,
      "lineHeight": 40
    },
    "xs": {
      "size": 12,
      "lineHeight": 16
    },
    "xxl": {
      "size": 48,
      "lineHeight": 52
    },
    "xxs": {
      "size": 10,
      "lineHeight": 14
    }
  },
  "fonts": {
    "ui": {
      "family": "\"GT Standard M\", -apple-system, BlinkMacSystemFont, sans-serif",
      "weights": {
        "heavy": 700,
        "medium": 500,
        "regular": 400
      }
    },
    "data": {
      "family": "\"Sidebar Geist Mono\", monospace",
      "weights": {
        "heavy": 700,
        "medium": 500,
        "regular": 400
      }
    },
    "brand": {
      "family": "\"GT Standard M\", -apple-system, BlinkMacSystemFont, sans-serif",
      "weights": {
        "heavy": 700,
        "medium": 500,
        "regular": 400
      }
    },
    "editorial": {
      "family": "\"GT Standard M\", -apple-system, BlinkMacSystemFont, sans-serif",
      "weights": {
        "heavy": 700,
        "medium": 500,
        "regular": 400
      }
    }
  },
  "border": {
    "l": 2,
    "m": 1,
    "s": 1,
    "none": 0
  },
  "radius": {
    "l": 17,
    "m": 9,
    "s": 6,
    "xl": 26,
    "xs": 3,
    "full": 9999,
    "zero": 0
  },
  "shadows": {
    "l": {
      "x": 0,
      "y": 12,
      "blur": 30,
      "color": {
        "dark": "neutral-1",
        "light": "neutral-10"
      },
      "spread": 0,
      "opacity": 0
    },
    "m": {
      "x": 0,
      "y": 8,
      "blur": 24,
      "color": {
        "dark": "neutral-1",
        "light": "neutral-10"
      },
      "spread": 0,
      "opacity": 0
    },
    "s": {
      "x": 0,
      "y": 1,
      "blur": 2,
      "color": {
        "dark": "neutral-1",
        "light": "neutral-10"
      },
      "spread": 0,
      "opacity": 0
    }
  },
  "spacing": {
    "l": 24,
    "m": 16,
    "s": 12,
    "xl": 32,
    "xs": 8,
    "xxl": 48,
    "xxs": 4,
    "zero": 0
  },
  "animation": {
    "large": {
      "easing": [
        0.16,
        1,
        0.3,
        1
      ],
      "duration": 300
    },
    "easing": [
      0.2,
      0.8,
      0.2,
      1
    ],
    "duration": 150,
    "popupScale": 0.96,
    "pressDistance": 1
  },
  "iconStyle": "outlined",
  "iconFamily": "Central",
  "letterSpacing": {
    "l": 0.01,
    "m": 0,
    "s": -0.01,
    "xl": 0.02,
    "xs": -0.02
  },
  "primaryForeground": {
    "dark": "neutral-2",
    "light": "neutral-2"
  },
  "primaryActionColor": "color-1"
}
```

## light CSS variables

Define these in the app’s existing theme scope for this mode. Keep component styles linked to the variables.

| Variable | Value |
| --- | --- |
| `--theme-name` | Untitled |
| `--theme-icon-family` | Central |
| `--theme-icon-style` | outlined |
| `--toolbar-divider-bleed` | 1 |
| `--focus-ring-outline` | 2px solid color-mix(in srgb, #000000 50%, transparent) |
| `--icon-stroke-width` | 2 |
| `--icon-light-display` | none |
| `--icon-regular-display` | inline |
| `--icon-bold-display` | none |
| `--motion-duration` | 150ms |
| `--motion-easing` | cubic-bezier(0.2, 0.8, 0.2, 1) |
| `--motion-type` | easing |
| `--motion-visual-duration` | 0.15 |
| `--motion-bounce` | 0.2 |
| `--motion-enabled` | 1 |
| `--motion-small-iterations` | infinite |
| `--motion-large-duration` | 300ms |
| `--motion-large-easing` | cubic-bezier(0.16, 1, 0.3, 1) |
| `--motion-large-type` | easing |
| `--motion-large-visual-duration` | 0.3 |
| `--motion-large-bounce` | 0.2 |
| `--motion-large-iterations` | infinite |
| `--motion-popup-scale` | 0.96 |
| `--motion-press-distance` | 1px |
| `--option-badge-background` | color-mix(in srgb, var(--color-1) 10%, transparent) |
| `--option-badge-foreground` | #262626 |
| `--navigation-active-foreground` | #fafafa |
| `--emphasis-chart-fill` | #26262633 |
| `--surface-raised-image` | none |
| `--surface-raised-shadow` | 0 0 0 0 transparent |
| `--surface-recessed-image` | none |
| `--surface-recessed-shadow` | 0 0 0 0 transparent |
| `--space-zero` | 0px |
| `--space-xxs` | 4px |
| `--space-xs` | 8px |
| `--space-s` | 12px |
| `--space-m` | 16px |
| `--space-l` | 24px |
| `--space-xl` | 32px |
| `--space-xxl` | 48px |
| `--tracking-xs` | -0.02em |
| `--tracking-s` | -0.01em |
| `--tracking-m` | 0em |
| `--tracking-l` | 0.01em |
| `--tracking-xl` | 0.02em |
| `--size-xxs` | 10px |
| `--line-xxs` | 14px |
| `--letter-spacing-xxs` | 0.02em |
| `--size-xs` | 12px |
| `--line-xs` | 16px |
| `--letter-spacing-xs` | 0.01em |
| `--size-s` | 14px |
| `--line-s` | 20px |
| `--letter-spacing-s` | 0.01em |
| `--size-m` | 16px |
| `--line-m` | 24px |
| `--letter-spacing-m` | 0em |
| `--size-l` | 24px |
| `--line-l` | 32px |
| `--letter-spacing-l` | -0.01em |
| `--size-xl` | 36px |
| `--line-xl` | 40px |
| `--letter-spacing-xl` | -0.01em |
| `--size-xxl` | 48px |
| `--line-xxl` | 52px |
| `--letter-spacing-xxl` | -0.02em |
| `--radius-zero` | 0px |
| `--radius-xs` | 3px |
| `--radius-s` | 6px |
| `--radius-m` | 9px |
| `--radius-l` | 17px |
| `--radius-xl` | 26px |
| `--radius-full` | 9999px |
| `--border-none` | 0px |
| `--border-s` | 1px |
| `--border-m` | 1px |
| `--border-l` | 2px |
| `--border-default-color` | rgb(0 0 0 / 0.1) |
| `--border-shadow-none` | 0 0 0 0 transparent |
| `--border-shadow-s` | 0 0 0 1px rgb(0 0 0 / 0.1) |
| `--border-shadow-m` | 0 0 0 1px rgb(0 0 0 / 0.1) |
| `--border-shadow-l` | 0 0 0 2px rgb(0 0 0 / 0.1) |
| `--font-ui` | "GT Standard M", -apple-system, BlinkMacSystemFont, sans-serif |
| `--weight-ui-regular` | 400 |
| `--weight-ui-medium` | 500 |
| `--weight-ui-heavy` | 700 |
| `--font-brand` | "GT Standard M", -apple-system, BlinkMacSystemFont, sans-serif |
| `--weight-brand-regular` | 400 |
| `--weight-brand-medium` | 500 |
| `--weight-brand-heavy` | 700 |
| `--font-editorial` | "GT Standard M", -apple-system, BlinkMacSystemFont, sans-serif |
| `--weight-editorial-regular` | 400 |
| `--weight-editorial-medium` | 500 |
| `--weight-editorial-heavy` | 700 |
| `--font-data` | "Sidebar Geist Mono", monospace |
| `--weight-data-regular` | 400 |
| `--weight-data-medium` | 500 |
| `--weight-data-heavy` | 700 |
| `--color-none` | transparent |
| `--color-1` | #262626 |
| `--color-1-transparent` | #26262633 |
| `--color-2` | #707070 |
| `--color-2-transparent` | #70707033 |
| `--color-3` | #b6b6b6 |
| `--color-3-transparent` | #b6b6b633 |
| `--color-4` | #e8e8e8 |
| `--color-4-transparent` | #e8e8e833 |
| `--neutral-1` | #ffffff |
| `--neutral-1-transparent` | #ffffff33 |
| `--neutral-2` | #fafafa |
| `--neutral-2-transparent` | #fafafa33 |
| `--neutral-3` | #f0f0f0 |
| `--neutral-3-transparent` | #f0f0f033 |
| `--neutral-4` | #dedede |
| `--neutral-4-transparent` | #dedede33 |
| `--neutral-5` | #c5c5c5 |
| `--neutral-5-transparent` | #c5c5c533 |
| `--neutral-6` | #969696 |
| `--neutral-6-transparent` | #96969633 |
| `--neutral-7` | #6c6c6c |
| `--neutral-7-transparent` | #6c6c6c33 |
| `--neutral-8` | #454545 |
| `--neutral-8-transparent` | #45454533 |
| `--neutral-9` | #252525 |
| `--neutral-9-transparent` | #25252533 |
| `--neutral-10` | #000000 |
| `--neutral-10-transparent` | #00000033 |
| `--success` | #76ef6b |
| `--success-transparent` | #76ef6b33 |
| `--warning` | #ffa344 |
| `--warning-transparent` | #ffa34433 |
| `--error` | #ff5263 |
| `--error-transparent` | #ff526333 |
| `--shadow-none` | none |
| `--shadow-s` | 0px 1px 2px 0px #00000000 |
| `--shadow-m` | 0px 8px 24px 0px #00000000 |
| `--shadow-l` | 0px 12px 30px 0px #00000000 |
| `--cte-canvas` | #ffffff |
| `--cte-surface` | #fafafa |
| `--cte-surface-muted` | #f0f0f0 |
| `--cte-text` | #000000 |
| `--cte-text-muted` | #6c6c6c |
| `--cte-border` | rgb(0 0 0 / 0.1) |
| `--cte-accent` | #262626 |
| `--cte-accent-text` | #fafafa |
| `--cte-danger` | #ff5263 |
| `--cte-focus` | #262626 |
| `--cte-font` | "GT Standard M", -apple-system, BlinkMacSystemFont, sans-serif |
| `--cte-font-size` | 16px |
| `--cte-font-weight` | 400 |
| `--cte-line-height` | 24px |
| `--cte-letter-spacing` | 0em |
| `--cte-detail-font-size` | 16px |
| `--cte-detail-line-height` | 24px |
| `--cte-detail-letter-spacing` | 0em |

## dark CSS variables

Define these in the app’s existing theme scope for this mode. Keep component styles linked to the variables.

| Variable | Value |
| --- | --- |
| `--theme-name` | Untitled |
| `--theme-icon-family` | Central |
| `--theme-icon-style` | outlined |
| `--toolbar-divider-bleed` | 1 |
| `--focus-ring-outline` | 2px solid color-mix(in srgb, #ffffff 50%, transparent) |
| `--icon-stroke-width` | 2 |
| `--icon-light-display` | none |
| `--icon-regular-display` | inline |
| `--icon-bold-display` | none |
| `--motion-duration` | 150ms |
| `--motion-easing` | cubic-bezier(0.2, 0.8, 0.2, 1) |
| `--motion-type` | easing |
| `--motion-visual-duration` | 0.15 |
| `--motion-bounce` | 0.2 |
| `--motion-enabled` | 1 |
| `--motion-small-iterations` | infinite |
| `--motion-large-duration` | 300ms |
| `--motion-large-easing` | cubic-bezier(0.16, 1, 0.3, 1) |
| `--motion-large-type` | easing |
| `--motion-large-visual-duration` | 0.3 |
| `--motion-large-bounce` | 0.2 |
| `--motion-large-iterations` | infinite |
| `--motion-popup-scale` | 0.96 |
| `--motion-press-distance` | 1px |
| `--option-badge-background` | color-mix(in srgb, var(--color-1) 10%, transparent) |
| `--option-badge-foreground` | #e5e5e5 |
| `--navigation-active-foreground` | #202020 |
| `--emphasis-chart-fill` | #e5e5e533 |
| `--surface-raised-image` | none |
| `--surface-raised-shadow` | 0 0 0 0 transparent |
| `--surface-recessed-image` | none |
| `--surface-recessed-shadow` | 0 0 0 0 transparent |
| `--space-zero` | 0px |
| `--space-xxs` | 4px |
| `--space-xs` | 8px |
| `--space-s` | 12px |
| `--space-m` | 16px |
| `--space-l` | 24px |
| `--space-xl` | 32px |
| `--space-xxl` | 48px |
| `--tracking-xs` | -0.02em |
| `--tracking-s` | -0.01em |
| `--tracking-m` | 0em |
| `--tracking-l` | 0.01em |
| `--tracking-xl` | 0.02em |
| `--size-xxs` | 10px |
| `--line-xxs` | 14px |
| `--letter-spacing-xxs` | 0.02em |
| `--size-xs` | 12px |
| `--line-xs` | 16px |
| `--letter-spacing-xs` | 0.01em |
| `--size-s` | 14px |
| `--line-s` | 20px |
| `--letter-spacing-s` | 0.01em |
| `--size-m` | 16px |
| `--line-m` | 24px |
| `--letter-spacing-m` | 0em |
| `--size-l` | 24px |
| `--line-l` | 32px |
| `--letter-spacing-l` | -0.01em |
| `--size-xl` | 36px |
| `--line-xl` | 40px |
| `--letter-spacing-xl` | -0.01em |
| `--size-xxl` | 48px |
| `--line-xxl` | 52px |
| `--letter-spacing-xxl` | -0.02em |
| `--radius-zero` | 0px |
| `--radius-xs` | 3px |
| `--radius-s` | 6px |
| `--radius-m` | 9px |
| `--radius-l` | 17px |
| `--radius-xl` | 26px |
| `--radius-full` | 9999px |
| `--border-none` | 0px |
| `--border-s` | 1px |
| `--border-m` | 1px |
| `--border-l` | 2px |
| `--border-default-color` | rgb(0 0 0 / 0.1) |
| `--border-shadow-none` | 0 0 0 0 transparent |
| `--border-shadow-s` | 0 0 0 1px rgb(0 0 0 / 0.1) |
| `--border-shadow-m` | 0 0 0 1px rgb(0 0 0 / 0.1) |
| `--border-shadow-l` | 0 0 0 2px rgb(0 0 0 / 0.1) |
| `--font-ui` | "GT Standard M", -apple-system, BlinkMacSystemFont, sans-serif |
| `--weight-ui-regular` | 400 |
| `--weight-ui-medium` | 500 |
| `--weight-ui-heavy` | 700 |
| `--font-brand` | "GT Standard M", -apple-system, BlinkMacSystemFont, sans-serif |
| `--weight-brand-regular` | 400 |
| `--weight-brand-medium` | 500 |
| `--weight-brand-heavy` | 700 |
| `--font-editorial` | "GT Standard M", -apple-system, BlinkMacSystemFont, sans-serif |
| `--weight-editorial-regular` | 400 |
| `--weight-editorial-medium` | 500 |
| `--weight-editorial-heavy` | 700 |
| `--font-data` | "Sidebar Geist Mono", monospace |
| `--weight-data-regular` | 400 |
| `--weight-data-medium` | 500 |
| `--weight-data-heavy` | 700 |
| `--color-none` | transparent |
| `--color-1` | #e5e5e5 |
| `--color-1-transparent` | #e5e5e533 |
| `--color-2` | #b1b1b1 |
| `--color-2-transparent` | #b1b1b133 |
| `--color-3` | #777777 |
| `--color-3-transparent` | #77777733 |
| `--color-4` | #424242 |
| `--color-4-transparent` | #42424233 |
| `--neutral-1` | #000000 |
| `--neutral-1-transparent` | #00000033 |
| `--neutral-2` | #202020 |
| `--neutral-2-transparent` | #20202033 |
| `--neutral-3` | #2b2b2b |
| `--neutral-3-transparent` | #2b2b2b33 |
| `--neutral-4` | #383838 |
| `--neutral-4-transparent` | #38383833 |
| `--neutral-5` | #505050 |
| `--neutral-5-transparent` | #50505033 |
| `--neutral-6` | #777777 |
| `--neutral-6-transparent` | #77777733 |
| `--neutral-7` | #a3a3a3 |
| `--neutral-7-transparent` | #a3a3a333 |
| `--neutral-8` | #c6c6c6 |
| `--neutral-8-transparent` | #c6c6c633 |
| `--neutral-9` | #e5e5e5 |
| `--neutral-9-transparent` | #e5e5e533 |
| `--neutral-10` | #ffffff |
| `--neutral-10-transparent` | #ffffff33 |
| `--success` | #76ef6b |
| `--success-transparent` | #76ef6b33 |
| `--warning` | #ffa344 |
| `--warning-transparent` | #ffa34433 |
| `--error` | #ff5263 |
| `--error-transparent` | #ff526333 |
| `--shadow-none` | none |
| `--shadow-s` | 0px 1px 2px 0px #00000000 |
| `--shadow-m` | 0px 8px 24px 0px #00000000 |
| `--shadow-l` | 0px 12px 30px 0px #00000000 |
| `--cte-canvas` | #000000 |
| `--cte-surface` | #202020 |
| `--cte-surface-muted` | #2b2b2b |
| `--cte-text` | #ffffff |
| `--cte-text-muted` | #a3a3a3 |
| `--cte-border` | rgb(0 0 0 / 0.1) |
| `--cte-accent` | #e5e5e5 |
| `--cte-accent-text` | #202020 |
| `--cte-danger` | #ff5263 |
| `--cte-focus` | #e5e5e5 |
| `--cte-font` | "GT Standard M", -apple-system, BlinkMacSystemFont, sans-serif |
| `--cte-font-size` | 16px |
| `--cte-font-weight` | 400 |
| `--cte-line-height` | 24px |
| `--cte-letter-spacing` | 0em |
| `--cte-detail-font-size` | 16px |
| `--cte-detail-line-height` | 24px |
| `--cte-detail-letter-spacing` | 0em |

## Authored component assignments

These are project edits. The [component reference](untitled-components.md) includes the effective assignments with defaults and shared parts resolved.

```json
{
  "componentTokens": {
    "switch:default:part:row:rest": {
      "gap": "s"
    },
    "switch:default:part:label:rest": {
      "textSize": "m"
    },
    "checkbox:default:part:label:rest": {
      "textSize": "m"
    },
    "switch:default:part:control:rest": {
      "controlSize": "xl"
    }
  },
  "componentVariants": {}
}
```
