// The only place a raw colour may appear: canvases cannot read CSS tokens, so they take theirs
// from here. Keep each value equal to its token in design/tokens.css.
export const ACCENT = '#2f6fff';

// Measure scales (DESIGN, Tokens). Each is defined once, in OKLCH, with even lightness steps in
// the neutral ramp's hue family (hue 250); canvas fills and legends both read it from here.
export type Theme = 'light' | 'dark';
export type ScaleKind = 'sequential' | 'diverging';
export type Rgb = readonly [number, number, number];

export type Swatch = {
  lightness: number;
  rgb: Rgb;
  css: string;
  inkRgb: Rgb;
  inkCss: string;
};

const RAMP_HUE = 250;
// The warm counterpart of the diverging scale: amber, opposite the ramp's blue.
const WARM_HUE = 65;

function srgbByte(linear: number): number {
  const v = linear <= 0.0031308 ? 12.92 * linear : 1.055 * Math.pow(linear, 1 / 2.4) - 0.055;
  return Math.round(Math.min(1, Math.max(0, v)) * 255);
}

function oklabToLinear(l: number, a: number, b: number): [number, number, number] {
  const lp = (l + 0.3963377774 * a + 0.2158037573 * b) ** 3;
  const mp = (l - 0.1055613458 * a - 0.0638541728 * b) ** 3;
  const sp = (l - 0.0894841775 * a - 1.291485548 * b) ** 3;
  return [
    4.0767416621 * lp - 3.3077115913 * mp + 0.2309699292 * sp,
    -1.2684380046 * lp + 2.6097574011 * mp - 0.3413193965 * sp,
    -0.0041960863 * lp - 0.7034186147 * mp + 1.707614701 * sp,
  ];
}

const inGamut = (c: number[]) => c.every((v) => v >= -0.0005 && v <= 1.0005);

// OKLCH to sRGB bytes; an out-of-gamut chroma is reduced (hue and lightness kept), not clipped.
function oklch(l: number, c: number, hue: number): Rgb {
  const rad = (hue * Math.PI) / 180;
  const at = (chroma: number) => oklabToLinear(l, chroma * Math.cos(rad), chroma * Math.sin(rad));
  let lin = at(c);
  if (!inGamut(lin)) {
    let lo = 0;
    let hi = c;
    for (let i = 0; i < 12; i++) {
      const mid = (lo + hi) / 2;
      if (inGamut(at(mid))) lo = mid;
      else hi = mid;
    }
    lin = at(lo);
  }
  return [srgbByte(lin[0]), srgbByte(lin[1]), srgbByte(lin[2])];
}

function luminance([r, g, b]: Rgb): number {
  const lin = (v: number) => {
    const s = v / 255;
    return s <= 0.04045 ? s / 12.92 : ((s + 0.055) / 1.055) ** 2.4;
  };
  return 0.2126 * lin(r) + 0.7152 * lin(g) + 0.0722 * lin(b);
}

/** WCAG contrast ratio of two colours. */
export function contrast(a: Rgb, b: Rgb): number {
  const [hi, lo] = [luminance(a), luminance(b)].sort((x, y) => y - x) as [number, number];
  return (hi + 0.05) / (lo + 0.05);
}

const css = ([r, g, b]: Rgb) => `rgb(${String(r)} ${String(g)} ${String(b)})`;

// Label ink: the ramp's darkest step or white, whichever reads better; the worst case over all
// fills is 4.5:1 (the crossover), which is why the dark ink is a little darker than --n-15.
const INK_DARK = oklch(0.1, 0.01, RAMP_HUE);
const INK_LIGHT = oklch(1, 0, 0);

function swatch(lightness: number, chroma: number, hue: number): Swatch {
  const rgb = oklch(lightness, chroma, hue);
  const inkRgb = contrast(rgb, INK_DARK) >= contrast(rgb, INK_LIGHT) ? INK_DARK : INK_LIGHT;
  return { lightness, rgb, css: css(rgb), inkRgb, inkCss: css(inkRgb) };
}

// Lightness runs [low, high] across the scale: low magnitudes are pale on paper and deep on the
// dark ground, so a bigger value is always the more prominent mark.
const LIGHTNESS = {
  light: { low: 0.94, high: 0.36 },
  dark: { low: 0.34, high: 0.88 },
} as const;

/** `steps` swatches, lowest value first. A diverging scale's middle is its pale (light) or deep (dark) neutral. */
export function scaleSwatches(kind: ScaleKind, steps: number, theme: Theme): Swatch[] {
  const { low, high } = LIGHTNESS[theme];
  return Array.from({ length: steps }, (_, i) => {
    const t = steps === 1 ? 0.5 : i / (steps - 1);
    if (kind === 'sequential') return swatch(low + (high - low) * t, 0.03 + 0.1 * Math.sin(Math.PI * t), RAMP_HUE);
    const away = Math.abs(2 * t - 1);
    return swatch(low + (high - low) * away, 0.012 + 0.11 * away, t < 0.5 ? RAMP_HUE : WARM_HUE);
  });
}

/** Fills for nodes with no colour step: a leaf, and a directory the leaves sit in. */
export function neutralSwatch(role: 'leaf' | 'group', theme: Theme): Swatch {
  const light = theme === 'light';
  if (role === 'leaf') return light ? swatch(0.87, 0.014, RAMP_HUE) : swatch(0.37, 0.014, RAMP_HUE);
  return light ? swatch(0.91, 0.012, RAMP_HUE) : swatch(0.27, 0.013, RAMP_HUE);
}
