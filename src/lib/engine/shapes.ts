export type Shape<T> = (value: unknown) => value is T;

export function isRecord(value: unknown): value is Record<string, unknown> {
  return typeof value === "object" && value !== null && !Array.isArray(value);
}

export function withKeys<T>(...keys: string[]): Shape<T> {
  return (value: unknown): value is T => isRecord(value) && keys.every((key) => key in value);
}

export function listOf<T>(): Shape<T[]> {
  return (value: unknown): value is T[] => Array.isArray(value);
}

export function isNull(value: unknown): value is null {
  return value === null;
}

export function isNumberRecord(value: unknown): value is Record<string, number> {
  return isRecord(value) && Object.values(value).every((v) => typeof v === "number");
}

export function isPair(value: unknown): value is [number, number] {
  return Array.isArray(value) && value.length === 2 && value.every((v) => typeof v === "number");
}

export function isPairList(value: unknown): value is [number, number][] {
  return Array.isArray(value) && value.every(isPair);
}

export function parsed<T>(text: string, accepts: Shape<T>): T {
  const value: unknown = JSON.parse(text);
  if (!accepts(value)) {
    throw new Error("The photo engine returned data in a shape this app doesn't expect.");
  }
  return value;
}
