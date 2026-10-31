// Colors for the theme editor: reading hex and rgb(), HSV for the picker,
// laying a see-through color over another, and WCAG contrast.

/** Red, green and blue from 0 to 255; alpha from 0 (clear) to 1 (solid). */
export interface Rgba {
  r: number;
  g: number;
  b: number;
  a: number;
}

/** Hue in degrees, saturation and brightness from 0 to 1. */
export interface Hsv {
  h: number;
  s: number;
  v: number;
}

const clamp = (n: number, lo: number, hi: number) => Math.min(hi, Math.max(lo, n));

/** "#rrggbb" or "#rrggbbaa". The # may be left out, as people often paste them. */
export function parseHex(text: string): Rgba | null {
  const m = /^#?([0-9a-f]{6})([0-9a-f]{2})?$/i.exec(text.trim());
  if (!m) return null;
  const n = parseInt(m[1], 16);
  return { r: (n >> 16) & 255, g: (n >> 8) & 255, b: n & 255, a: m[2] ? parseInt(m[2], 16) / 255 : 1 };
}

/** Hex, or CSS rgb()/rgba() with commas or spaces and an optional alpha. */
export function parseColor(text: string): Rgba | null {
  const hex = parseHex(text);
  if (hex) return hex;
  const m = /^rgba?\(\s*([\d.]+)[\s,]+([\d.]+)[\s,]+([\d.]+)\s*(?:[,/]\s*([\d.]+)(%?)\s*)?\)$/i.exec(text.trim());
  if (!m) return null;
  const [r, g, b] = [m[1], m[2], m[3]].map(Number);
  if ([r, g, b].some((n) => n > 255)) return null;
  const a = m[4] === undefined ? 1 : Number(m[4]) / (m[5] ? 100 : 1);
  return a > 1 ? null : { r, g, b, a };
}

const byte = (n: number) => Math.round(clamp(n, 0, 255)).toString(16).padStart(2, "0");

/** "#rrggbb", or "#rrggbbaa" when the color is see-through. Lower case. */
export function toHex({ r, g, b, a }: Rgba): string {
  const alpha = Math.round(clamp(a, 0, 1) * 255);
  return `#${byte(r)}${byte(g)}${byte(b)}${alpha < 255 ? byte(alpha) : ""}`;
}

/** The color you see where `top` lies over `bottom`. */
export function over(top: Rgba, bottom: Rgba): Rgba {
  const a = top.a + bottom.a * (1 - top.a);
  if (a === 0) return { r: 0, g: 0, b: 0, a: 0 };
  const mix = (t: number, b: number) => (t * top.a + b * bottom.a * (1 - top.a)) / a;
  return { r: mix(top.r, bottom.r), g: mix(top.g, bottom.g), b: mix(top.b, bottom.b), a };
}

/** WCAG relative luminance: 0 for black, 1 for white. Ignores alpha. */
export function luminance({ r, g, b }: Rgba): number {
  const lin = (c: number) => {
    const v = c / 255;
    return v <= 0.03928 ? v / 12.92 : ((v + 0.055) / 1.055) ** 2.4;
  };
  return 0.2126 * lin(r) + 0.7152 * lin(g) + 0.0722 * lin(b);
}

/** WCAG contrast ratio between two solid colors, from 1 to 21. 4.5 reads well as body text. */
export function contrast(a: Rgba, b: Rgba): number {
  const [hi, lo] = [luminance(a), luminance(b)].sort((x, y) => y - x);
  return (hi + 0.05) / (lo + 0.05);
}

/** Whether black text reads better on it than white: a light color. */
export function isLightColor(c: Rgba): boolean {
  const l = luminance(c);
  return (l + 0.05) / 0.05 > 1.05 / (l + 0.05);
}

export function rgbToHsv({ r, g, b }: Rgba): Hsv {
  const [R, G, B] = [r / 255, g / 255, b / 255];
  const max = Math.max(R, G, B);
  const d = max - Math.min(R, G, B);
  let h = 0;
  if (d) {
    if (max === R) h = ((G - B) / d) % 6;
    else if (max === G) h = (B - R) / d + 2;
    else h = (R - G) / d + 4;
    h = (h * 60 + 360) % 360;
  }
  return { h, s: max ? d / max : 0, v: max };
}

export function hsvToRgb({ h, s, v }: Hsv, a = 1): Rgba {
  const f = (n: number) => {
    const k = (n + h / 60) % 6;
    return Math.round((v - v * s * Math.max(0, Math.min(k, 4 - k, 1))) * 255);
  };
  return { r: f(5), g: f(3), b: f(1), a };
}
