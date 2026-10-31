import { describe, expect, it } from 'vitest';
import { contrast, neutralSwatch, scaleSwatches, type ScaleKind, type Theme } from './palette';

const kinds: ScaleKind[] = ['sequential', 'diverging'];
const themes: Theme[] = ['light', 'dark'];
const stepCounts = [1, 2, 3, 5, 7, 9, 12];

describe('palette', () => {
  it('keeps label ink at 4.5:1 on every step in both themes', () => {
    for (const theme of themes) {
      const all = [
        neutralSwatch('leaf', theme),
        neutralSwatch('group', theme),
        ...kinds.flatMap((kind) => stepCounts.flatMap((n) => scaleSwatches(kind, n, theme))),
      ];
      for (const s of all) expect(contrast(s.rgb, s.inkRgb)).toBeGreaterThanOrEqual(4.5);
    }
  });

  it('steps the sequential scale monotonically and evenly in lightness', () => {
    for (const theme of themes) {
      for (const n of stepCounts.filter((c) => c > 2)) {
        const l = scaleSwatches('sequential', n, theme).map((s) => s.lightness);
        const deltas = l.slice(1).map((v, i) => v - (l[i] ?? 0));
        const sign = Math.sign(deltas[0] ?? 0);
        for (const d of deltas) {
          expect(Math.sign(d)).toBe(sign);
          expect(d).toBeCloseTo(deltas[0] ?? 0, 9);
        }
      }
    }
  });

  it('puts the diverging midpoint at the extreme of lightness, symmetric about it', () => {
    for (const theme of themes) {
      const l = scaleSwatches('diverging', 9, theme).map((s) => s.lightness);
      const mid = l[4] ?? 0;
      for (let i = 0; i < 4; i++) {
        expect(l[i]).toBeCloseTo(l[8 - i] ?? 0, 9);
        expect(Math.abs((l[i] ?? 0) - mid)).toBeGreaterThan(Math.abs((l[i + 1] ?? 0) - mid));
      }
    }
  });
});
