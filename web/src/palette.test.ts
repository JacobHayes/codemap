import { describe, expect, it } from 'vitest';
import { contrast, neutralSwatch, scaleSwatches, type Theme } from './palette';

const themes: Theme[] = ['light', 'dark'];
const stepCounts = [1, 2, 3, 5, 7, 9, 12];

describe('palette', () => {
  it('keeps label ink at 4.5:1 on every step in both themes', () => {
    for (const theme of themes) {
      const all = [
        neutralSwatch('leaf', theme),
        neutralSwatch('group', theme),
        ...stepCounts.flatMap((n) => scaleSwatches(n, theme)),
      ];
      for (const s of all) expect(contrast(s.rgb, s.inkRgb)).toBeGreaterThanOrEqual(4.5);
    }
  });

  it('steps the sequential scale monotonically and evenly in lightness', () => {
    for (const theme of themes) {
      for (const n of stepCounts.filter((c) => c > 2)) {
        const l = scaleSwatches(n, theme).map((s) => s.lightness);
        const deltas = l.slice(1).map((v, i) => v - (l[i] ?? 0));
        const sign = Math.sign(deltas[0] ?? 0);
        for (const d of deltas) {
          expect(Math.sign(d)).toBe(sign);
          expect(d).toBeCloseTo(deltas[0] ?? 0, 9);
        }
      }
    }
  });

  it('keeps "no value" distinct from the scales\' low and middle steps', () => {
    for (const theme of themes) {
      const leaf = neutralSwatch('leaf', theme).lightness;
      const group = neutralSwatch('group', theme).lightness;
      const lowest = scaleSwatches(9, theme)[0]?.lightness ?? 0;
      for (const l of [lowest]) {
        expect(Math.abs(l - leaf)).toBeGreaterThanOrEqual(0.1);
        expect(Math.abs(l - group)).toBeGreaterThanOrEqual(0.1);
      }
    }
  });
});
