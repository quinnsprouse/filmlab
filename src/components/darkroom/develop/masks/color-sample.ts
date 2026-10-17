function encode(channel: number): number {
  const clamped = Math.min(1, Math.max(0, channel));
  const value = clamped <= 0.0031308 ? clamped * 12.92 : 1.055 * clamped ** (1 / 2.4) - 0.055;
  return Math.round(value * 255);
}

export function swatchColor([lightness, a, b]: [number, number, number]): string {
  const l = (lightness + 0.3963377774 * a + 0.2158037573 * b) ** 3;
  const m = (lightness - 0.1055613458 * a - 0.0638541728 * b) ** 3;
  const s = (lightness - 0.0894841775 * a - 1.291485548 * b) ** 3;
  const r = encode(4.0767416621 * l - 3.3077115913 * m + 0.2309699292 * s);
  const g = encode(-1.2684380046 * l + 2.6097574011 * m - 0.3413193965 * s);
  const blue = encode(-0.0041960863 * l - 0.7034186147 * m + 1.707614701 * s);
  return `rgb(${r} ${g} ${blue})`;
}
