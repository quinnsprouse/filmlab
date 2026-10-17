import * as z from "zod/mini";

function blankToUndefined(value: unknown) {
  return typeof value === "string" && value.trim() === "" ? undefined : value;
}

// why: Vite inlines VITE_* values into the client bundle, so a secret here becomes public.
const clientEnvSchema = z.object({
  VITE_APP_URL: z.pipe(
    z.transform(blankToUndefined),
    z.optional(
      z.pipe(
        z.url({ protocol: /^https?$/ }),
        z.transform((url) => new URL(url).origin),
      ),
    ),
  ),
});

function parseClientEnv(env: Record<string, unknown>) {
  const result = clientEnvSchema.safeParse(env);
  if (!result.success) {
    throw new Error(`Invalid environment variables:\n${z.prettifyError(result.error)}`);
  }
  return result.data;
}

export const clientEnv = parseClientEnv(import.meta.env);
