import fc from 'fast-check';
import { describe, expect, it } from 'vitest';
import {
  CHAR_PX,
  DEPTH_MAX,
  LABEL_PX,
  labelCells,
  layout,
  type Scene,
  type TreeInput,
} from './layout';
import { paint } from './paint';
import { pick } from './pick';

const SCALE = { kind: 'sequential', steps: 7, measure: 'lines', min: 0, max: 100 } as const;

// A tree is generated as a parent pointer per node (to an earlier node), so parents precede children.
type Spec = { parents: number[]; sizes: number[]; steps: (number | null)[] };

// A generated parent index is clamped to an earlier node.
const parentOf = (p: number, i: number) => Math.max(0, Math.min(p, i - 1));

function tree(spec: Spec): TreeInput {
  const hasChild = new Set(spec.parents.map((p, i) => (i === 0 ? -1 : parentOf(p, i))));
  const nodes = spec.sizes.map((size, i) => ({
    id: 100 + i,
    parent: i === 0 ? null : 100 + parentOf(spec.parents[i] ?? 0, i),
    name: `${i % 3 ? 'node' : '日本語🎉'}-${String(i)}-with-a-longer-name`,
    size: hasChild.has(i) ? 0 : size,
    colorStep: spec.steps[i] ?? null,
  }));
  return { nodes, scale: SCALE };
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
    let depth = 0;
    for (let p = s.parent[i] ?? -1; p >= 0; p = s.parent[p] ?? -1) depth++;
    expect(depth).toBeLessThanOrEqual(DEPTH_MAX);
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
    it(`keeps ${name} trees inside their parents, disjoint, shallow, with labels that fit`, () => {
      fc.assert(
        fc.property(specOf(60, deep), (spec) => {
          const s = layout(tree(spec), W, H, null);
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
          const nodes: TreeInput['nodes'] = [
            { id: 0, parent: null, name: 'r', size: 0, colorStep: null },
          ];
          sizes.forEach((sz, i) =>
            nodes.push({ id: i + 1, parent: 0, name: 'f', size: sz, colorStep: null }),
          );
          const s = layout({ nodes, scale: SCALE }, W, H, null);
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

  it('aggregates levels below the depth limit into their ancestor tile until drilled', () => {
    const nodes: TreeInput['nodes'] = [
      { id: 0, parent: null, name: 'root', size: 0, colorStep: null },
    ];
    // A file beside the first directory keeps the root from collapsing with it.
    nodes.push({ id: 200, parent: 0, name: 'readme', size: 10, colorStep: null });
    for (let d = 1; d <= 8; d++) {
      nodes.push({ id: d, parent: d - 1, name: `dir${String(d)}`, size: 0, colorStep: null });
      nodes.push({ id: 100 + d, parent: d, name: `file${String(d)}`, size: 10, colorStep: null });
    }
    const s = layout({ nodes, scale: SCALE }, W, H, null);
    const tiles = Array.from({ length: s.count }, (_, i) => i).filter((i) => s.isTile[i]);
    expect(tiles.every((i) => !s.canDrill[i] || s.nodeId[i] === DEPTH_MAX)).toBe(true);
    const aggregate = Array.from(s.nodeId).indexOf(DEPTH_MAX);
    expect(s.canDrill[aggregate]).toBe(1);
    expect(s.labels.find((l) => l.node === aggregate)?.value).toBe(
      (10 * (8 - DEPTH_MAX + 1)).toLocaleString('en-US'),
    );
    const drilled = layout({ nodes, scale: SCALE }, W, H, DEPTH_MAX);
    expect(Array.from(drilled.nodeId)).toContain(DEPTH_MAX + 1);
  });
});

describe('layout, continued', () => {
  it('drills into a subtree and fills the whole canvas with it', () => {
    const input = tree({ parents: [0, 0, 1, 1, 0], sizes: [0, 0, 5, 7, 9], steps: [] });
    const s = layout(input, 300, 200, 101);
    expect(s.nodeId[0]).toBe(101);
    expect([s.x0[0], s.y0[0], s.x1[0], s.y1[0]]).toEqual([0, 0, 300, 200]);
    expect(Array.from(s.nodeId).sort()).toEqual([101, 102, 103]);
  });

  it('collapses a single-child directory chain into one labelled band', () => {
    const nodes: TreeInput['nodes'] = [
      { id: 0, parent: null, name: 'alpha', size: 0, colorStep: null },
      { id: 1, parent: 0, name: 'beta', size: 0, colorStep: null },
      { id: 2, parent: 1, name: 'gamma', size: 0, colorStep: null },
      { id: 3, parent: 2, name: 'f1', size: 5, colorStep: null },
      { id: 4, parent: 2, name: 'f2', size: 5, colorStep: null },
    ];
    const s = layout({ nodes, scale: SCALE }, 600, 300, null);
    expect(Array.from(s.nodeId)).toEqual([2, 3, 4]);
    expect(s.labels[0]?.name).toBe('alpha/beta/gamma');
    const narrow = layout({ nodes, scale: SCALE }, 100, 300, null);
    expect(narrow.labels[0]?.name.startsWith('…')).toBe(true);
    expect(narrow.labels[0]?.name.endsWith('gamma')).toBe(true);
  });

  it('survives an empty or all-zero tree', () => {
    const zero = tree({ parents: [0, 0], sizes: [0, 0], steps: [] });
    expect(layout(zero, W, H, null).count).toBe(0);
    expect(layout({ nodes: [], scale: SCALE }, W, H, null).count).toBe(0);
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
        const s = layout(tree(spec), 400, 300, null);
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
      const nodes: TreeInput['nodes'] = [
        { id: 0, parent: null, name: 'root', size: 0, colorStep: null },
      ];
      let seed = 7;
      const rand = () => (seed = (seed * 1664525 + 1013904223) >>> 0) / 2 ** 32;
      for (let d = 1; d <= dirs; d++) {
        nodes.push({ id: d, parent: 0, name: `d${String(d)}`, size: 0, colorStep: null });
      }
      for (let f = 0; f < leaves; f++) {
        nodes.push({
          id: dirs + 1 + f,
          parent: 1 + (f % dirs),
          name: `f${String(f)}.rs`,
          size: Math.ceil(rand() ** 4 * 5000),
          colorStep: Math.floor(rand() * 7),
        });
      }
      const t0 = performance.now();
      const s = layout({ nodes, scale: SCALE }, 1600, 1000, null);
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
