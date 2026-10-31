import fc from 'fast-check';
import { describe, expect, it } from 'vitest';
import type { Scale, Tree, TreeNode } from '../../generated/Answer';
import { CHAR_PX, LABEL_PX, labelCells, layout, type Scene } from './layout';
import { neighbour } from './nav';
import { paint } from './paint';
import { pick } from './pick';

const SCALE: Scale = {
  spacing: 'linear',
  steps: 7,
  measure: 'lines',
  min: 0,
  max: 100,
  edges: [0, 14, 28, 42, 57, 71, 85, 100],
};

// A payload from nodes given as [id, parent, name, own size, colour step]: totals, kinds and
// paths are derived the way the server does.
type Row = [number, number | null, string, number, number | null];
function payload(rows: Row[]): Tree {
  const total = new Map<number, number>();
  for (const [id, , , size] of rows) total.set(id, size);
  for (const [id, parent] of [...rows].reverse()) {
    if (parent !== null) total.set(parent, (total.get(parent) ?? 0) + (total.get(id) ?? 0));
  }
  const parents = new Set(rows.map(([, parent]) => parent));
  const nodes: TreeNode[] = rows.map(([id, parent, name, size, colorStep]) => ({
    id,
    parent,
    name,
    size,
    total: total.get(id) ?? 0,
    colorStep,
    value: null,
    kind: parents.has(id) ? 'directory' : 'file',
    path: name,
    hidden: 0,
  }));
  return {
    hierarchy: 'path',
    depth: 3,
    size: { measure: 'lines', unit: 'lines' },
    total: total.get(rows[0]?.[0] ?? 0) ?? 0,
    nodes,
    scale: SCALE,
    hidden: null,
  };
}

// A tree is generated as a parent pointer per node (to an earlier node), so parents precede children.
type Spec = { parents: number[]; sizes: number[]; steps: (number | null)[] };

// A generated parent index is clamped to an earlier node.
const parentOf = (p: number, i: number) => Math.max(0, Math.min(p, i - 1));

function tree(spec: Spec): Tree {
  const hasChild = new Set(spec.parents.map((p, i) => (i === 0 ? -1 : parentOf(p, i))));
  return payload(
    spec.sizes.map((size, i) => [
      i,
      i === 0 ? null : parentOf(spec.parents[i] ?? 0, i),
      `${i % 3 ? 'node' : '日本語🎉'}-${String(i)}-with-a-longer-name`,
      hasChild.has(i) ? 0 : size,
      spec.steps[i] ?? null,
    ]),
  );
}

const size = fc.oneof(
  { weight: 1, arbitrary: fc.constant(0) },
  { weight: 6, arbitrary: fc.integer({ min: 1, max: 5000 }) },
);
const specOf = (maxNodes: number, deep: boolean): fc.Arbitrary<Spec> =>
  fc.integer({ min: 2, max: maxNodes }).chain((n) =>
    fc.record({
      // deep: each node hangs off the previous few, making long chains; otherwise anywhere earlier.
      parents: fc
        .array(fc.nat(deep ? 2 : n), { minLength: n, maxLength: n })
        .map((a) => a.map((v, i) => (deep ? i - 1 - v : v))),
      sizes: fc.array(size, { minLength: n, maxLength: n }),
      steps: fc.array(fc.option(fc.integer({ min: 0, max: 6 }), { nil: null }), {
        minLength: n,
        maxLength: n,
      }),
    }),
  );

const W = 1200;
const H = 800;
const EPS = 1e-6;

function checkGeometry(s: Scene) {
  for (let i = 0; i < s.count; i++) {
    for (const v of [s.x0[i], s.y0[i], s.x1[i], s.y1[i]]) expect(Number.isFinite(v)).toBe(true);
    expect(s.x1[i]).toBeGreaterThan(s.x0[i] ?? 0);
    expect(s.y1[i]).toBeGreaterThan(s.y0[i] ?? 0);
    const p = s.parent[i] ?? -1;
    if (p >= 0) {
      expect(s.x0[i]).toBeGreaterThanOrEqual((s.x0[p] ?? 0) - EPS);
      expect(s.y0[i]).toBeGreaterThanOrEqual((s.y0[p] ?? 0) - EPS);
      expect(s.x1[i]).toBeLessThanOrEqual((s.x1[p] ?? 0) + EPS);
      expect(s.y1[i]).toBeLessThanOrEqual((s.y1[p] ?? 0) + EPS);
    }
  }
}

function childrenOf(s: Scene): Map<number, number[]> {
  const kids = new Map<number, number[]>();
  for (let i = 1; i < s.count; i++) {
    const p = s.parent[i] ?? 0;
    kids.set(p, [...(kids.get(p) ?? []), i]);
  }
  return kids;
}

const area = (s: Scene, i: number) =>
  ((s.x1[i] ?? 0) - (s.x0[i] ?? 0)) * ((s.y1[i] ?? 0) - (s.y0[i] ?? 0));

// Compare all sibling pairs, which is fine at property-test sizes.
function checkDisjoint(s: Scene) {
  for (const group of childrenOf(s).values()) {
    for (let a = 0; a < group.length; a++) {
      for (let b = a + 1; b < group.length; b++) {
        const [i, j] = [group[a] ?? 0, group[b] ?? 0];
        const w = Math.min(s.x1[i] ?? 0, s.x1[j] ?? 0) - Math.max(s.x0[i] ?? 0, s.x0[j] ?? 0);
        const h = Math.min(s.y1[i] ?? 0, s.y1[j] ?? 0) - Math.max(s.y0[i] ?? 0, s.y0[j] ?? 0);
        expect(w <= EPS || h <= EPS).toBe(true);
      }
    }
  }
}

// A label fits its tile: width by its cells, height by its lines.
function checkLabels(s: Scene) {
  for (const l of s.labels) {
    const i = l.node;
    expect(l.x + labelCells(l.name) * CHAR_PX).toBeLessThanOrEqual((s.x1[i] ?? 0) + EPS);
    const bottom = l.y + (l.value === null ? 0 : 16) + LABEL_PX / 2;
    expect(bottom).toBeLessThanOrEqual((s.y1[i] ?? 0) + EPS);
    expect(l.y - LABEL_PX / 2).toBeGreaterThanOrEqual((s.y0[i] ?? 0) - EPS);
    if (l.value !== null)
      expect(l.x + l.value.length * CHAR_PX).toBeLessThanOrEqual((s.x1[i] ?? 0) + EPS);
  }
}

describe('layout', () => {
  for (const [name, deep] of [
    ['wide', false],
    ['deep', true],
  ] as const) {
    it(`keeps ${name} trees inside their parents, disjoint, with labels that fit`, () => {
      fc.assert(
        fc.property(specOf(60, deep), (spec) => {
          const s = layout(tree(spec), W, H);
          checkGeometry(s);
          checkDisjoint(s);
          checkLabels(s);
        }),
        { numRuns: 150 },
      );
    });
  }

  it('gives a flat tree tile areas proportional to size, within the gutters', () => {
    fc.assert(
      fc.property(
        fc.array(fc.integer({ min: 1, max: 1000 }), { minLength: 1, maxLength: 40 }),
        (sizes) => {
          const s = layout(
            payload([
              [0, null, 'r', 0, null],
              ...sizes.map((sz, i): Row => [i + 1, 0, 'f', sz, null]),
            ]),
            W,
            H,
          );
          const total = sizes.reduce((a, b) => a + b, 0);
          const tiles = Array.from({ length: s.count - 1 }, (_, k) => k + 1);
          const tileSum = tiles.reduce((a, i) => a + area(s, i), 0);
          for (const i of tiles) {
            const share = (sizes[(s.nodeId[i] ?? 1) - 1] ?? 0) / total;
            expect(Math.abs(area(s, i) / tileSum - share)).toBeLessThan(0.02);
          }
        },
      ),
    );
  });
});

describe('layout labels and chains', () => {
  it('labels a tile with its total and unit, and a short wide tile with its name alone', () => {
    const rows: Row[] = [
      [0, null, 'root', 0, null],
      [1, 0, 'big.rs', 303_000, null],
      [2, 0, 'small.rs', 700, null],
    ];
    const s = layout(payload(rows), 800, 400);
    expect(s.labels.find((l) => s.nodeId[l.node] === 1)?.value).toBe('303k lines');
    const flat = layout(payload(rows), 800, 40);
    const named = flat.labels.filter((l) => l.node > 0);
    expect(named.length).toBeGreaterThan(0);
    expect(named.every((l) => l.value === null)).toBe(true);
  });

  it('marks directories as drillable, files not', () => {
    const s = layout(
      payload([
        [0, null, 'root', 0, null],
        [1, 0, 'dir', 0, null],
        [2, 1, 'a.rs', 5, null],
        [3, 0, 'b.rs', 9, null],
      ]),
      600,
      300,
    );
    for (let i = 1; i < s.count; i++) {
      expect(s.canDrill[i]).toBe(s.nodeId[i] === 1 ? 1 : 0);
    }
  });

  it('collapses a single-child directory chain into one labelled band', () => {
    const rows: Row[] = [
      [0, null, 'alpha', 0, null],
      [1, 0, 'beta', 0, null],
      [2, 1, 'gamma', 0, null],
      [3, 2, 'f1', 5, null],
      [4, 2, 'f2', 5, null],
    ];
    const s = layout(payload(rows), 600, 300);
    expect(Array.from(s.nodeId)).toEqual([2, 3, 4]);
    expect(s.labels[0]?.name).toBe('alpha/beta/gamma');
    const narrow = layout(payload(rows), 100, 300);
    expect(narrow.labels[0]?.name.startsWith('…')).toBe(true);
    expect(narrow.labels[0]?.name.endsWith('gamma')).toBe(true);
  });

  it('survives an empty or all-zero tree', () => {
    const zero = tree({ parents: [0, 0], sizes: [0, 0], steps: [] });
    expect(layout(zero, W, H).count).toBe(0);
    expect(layout({ ...zero, nodes: [], scale: null }, W, H).count).toBe(0);
  });
});

describe('keyboard neighbours', () => {
  it('moves to a sibling in the direction asked, and stays among siblings', () => {
    fc.assert(
      fc.property(specOf(40, false), (spec) => {
        const s = layout(tree(spec), W, H);
        for (let from = 1; from < s.count; from++) {
          for (const direction of ['left', 'right', 'up', 'down'] as const) {
            const to = neighbour(s, from, direction);
            if (to === null) continue;
            expect(s.parent[to]).toBe(s.parent[from]);
            const cx = (i: number) => ((s.x0[i] ?? 0) + (s.x1[i] ?? 0)) / 2;
            const cy = (i: number) => ((s.y0[i] ?? 0) + (s.y1[i] ?? 0)) / 2;
            const moved = { left: cx(from) - cx(to), right: cx(to) - cx(from) };
            const down = { up: cy(from) - cy(to), down: cy(to) - cy(from) };
            const along = direction === 'left' || direction === 'right' ? moved : down;
            expect(along[direction as keyof typeof along]).toBeGreaterThan(0);
          }
        }
      }),
      { numRuns: 40 },
    );
  });

  it('selects the first tile when nothing is selected', () => {
    const s = layout(tree({ parents: [0, 0, 0], sizes: [0, 5, 9], steps: [] }), W, H);
    expect(neighbour(s, null, 'right')).toBe(1);
  });
});

// A stand-in context that records fills, so paint is checked without a browser.
function recorder() {
  const fills: [number, number, number, number][] = [];
  const ctx = {
    canvas: { width: 400, height: 300 },
    fillRect: (x: number, y: number, w: number, h: number) => fills.push([x, y, w, h]),
    clearRect: () => undefined,
    setTransform: () => undefined,
    fillText: () => undefined,
  };
  return { fills, ctx: ctx as unknown as CanvasRenderingContext2D };
}

describe('paint and pick', () => {
  it('fills one rect per tile inside the canvas and picks the tile under a point', () => {
    fc.assert(
      fc.property(specOf(80, false), fc.constantFrom(1, 2), (spec, scale) => {
        const s = layout(tree(spec), 400, 300);
        const { fills, ctx } = recorder();
        paint(ctx, s, 'light', scale);
        expect(fills).toHaveLength(s.count);
        for (const [x, y, w, h] of fills) {
          expect(x).toBeGreaterThanOrEqual(0);
          expect(y).toBeGreaterThanOrEqual(0);
          expect(x + w).toBeLessThanOrEqual(400 * scale);
          expect(y + h).toBeLessThanOrEqual(300 * scale);
        }
        for (let i = 0; i < s.count; i++) {
          if (!s.isTile[i]) continue;
          const x = ((s.x0[i] ?? 0) + (s.x1[i] ?? 0)) / 2;
          const y = ((s.y0[i] ?? 0) + (s.y1[i] ?? 0)) / 2;
          expect(pick(s, x, y)).toBe(i);
        }
        expect(pick(s, -1, 5)).toBeNull();
        expect(pick(s, 5, 300)).toBeNull();
      }),
      { numRuns: 40 },
    );
  });

  it('keeps 10k leaves valid and reports 100k timings', () => {
    // Leaves sit in a few directories, as in a real tree; deeper levels would aggregate away.
    for (const [leaves, dirs] of [
      [10_000, 20],
      [100_000, 50],
    ] as const) {
      const rows: Row[] = [[0, null, 'root', 0, null]];
      let seed = 7;
      const rand = () => (seed = (seed * 1664525 + 1013904223) >>> 0) / 2 ** 32;
      for (let d = 1; d <= dirs; d++) {
        rows.push([d, 0, `d${String(d)}`, 0, null]);
      }
      for (let f = 0; f < leaves; f++) {
        rows.push([
          dirs + 1 + f,
          1 + (f % dirs),
          `f${String(f)}.rs`,
          Math.ceil(rand() ** 4 * 5000),
          Math.floor(rand() * 7),
        ]);
      }
      const t0 = performance.now();
      const s = layout(payload(rows), 1600, 1000);
      const t1 = performance.now();
      const { fills, ctx } = recorder();
      paint(ctx, s, 'dark', 1);
      const t2 = performance.now();
      console.info(
        `${String(leaves)} leaves: layout ${(t1 - t0).toFixed(0)}ms, paint loop ${(t2 - t1).toFixed(0)}ms, ${String(s.count)} tiles, ${String(s.labels.length)} labels`,
      );
      checkLabels(s);
      expect(fills).toHaveLength(s.count);
    }
  }, 60_000);
});
