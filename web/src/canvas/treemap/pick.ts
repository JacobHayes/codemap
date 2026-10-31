import type { Scene } from './layout';

/** The deepest tile under a CSS-px point, or null outside the root. A scan from the back is enough
 * at a few thousand tiles: later tiles are deeper, and tiles at one depth never overlap. */
export function pick(scene: Scene, x: number, y: number): number | null {
  for (let i = scene.count - 1; i >= 0; i--) {
    if (
      x >= (scene.x0[i] ?? 0) &&
      x < (scene.x1[i] ?? 0) &&
      y >= (scene.y0[i] ?? 0) &&
      y < (scene.y1[i] ?? 0)
    )
      return i;
  }
  return null;
}

/** The path of a tile from the drill root, joined with slashes. */
export function pathOf(scene: Scene, i: number): string {
  const names: string[] = [];
  for (let p = i; p >= 0; p = scene.parent[p] ?? -1) names.unshift(scene.name[p] ?? '');
  return names.join('/');
}
