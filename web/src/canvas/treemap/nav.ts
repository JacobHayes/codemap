import type { Scene } from './layout';

export type Direction = 'left' | 'right' | 'up' | 'down';

const centre = (scene: Scene, i: number) => ({
  x: ((scene.x0[i] ?? 0) + (scene.x1[i] ?? 0)) / 2,
  y: ((scene.y0[i] ?? 0) + (scene.y1[i] ?? 0)) / 2,
});

// How far off the direction's axis a sibling may sit before it is a worse choice than a nearer
// one: moving across counts double.
const OFF_AXIS_WEIGHT = 2;

/** The sibling tile next to `from` in a direction, or null when there is none that way. With no
 * selection, any arrow selects the first (largest) tile under the root. */
export function neighbour(scene: Scene, from: number | null, direction: Direction): number | null {
  if (from === null) return scene.count > 1 ? 1 : null;
  const here = centre(scene, from);
  const horizontal = direction === 'left' || direction === 'right';
  const sign = direction === 'right' || direction === 'down' ? 1 : -1;
  let best: number | null = null;
  let bestCost = Infinity;
  for (let i = 1; i < scene.count; i++) {
    if (i === from || scene.parent[i] !== scene.parent[from]) continue;
    const there = centre(scene, i);
    const along = (horizontal ? there.x - here.x : there.y - here.y) * sign;
    const across = Math.abs(horizontal ? there.y - here.y : there.x - here.x);
    if (along <= 0) continue;
    const cost = along + OFF_AXIS_WEIGHT * across;
    if (cost < bestCost) {
      best = i;
      bestCost = cost;
    }
  }
  return best;
}
