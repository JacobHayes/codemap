import {
  ACCENT,
  neutralSwatch,
  outlineCss,
  scaleSwatches,
  type Swatch,
  type Theme,
} from '../../palette';
import { LABEL_FONT, type Scene } from './layout';

// Tiles are flat fills (DESIGN, Views): no shading, borders or gradients. Edges snap to device
// pixels so neighbouring tiles leave an even gutter.
function fillOf(scene: Scene, theme: Theme, i: number, steps: Swatch[]): Swatch {
  const step = scene.colorStep[i] ?? -1;
  // A group keeps its neutral tone, so nesting reads from the tiles' colours against it.
  if (step >= 0 && scene.isTile[i])
    return steps[Math.min(step, steps.length - 1)] ?? neutralSwatch('leaf', theme);
  return neutralSwatch(scene.isTile[i] ? 'leaf' : 'group', theme);
}

/** Fills and labels, parents first so children draw over their group. */
export function paint(
  ctx: CanvasRenderingContext2D,
  scene: Scene,
  theme: Theme,
  scale: number,
): void {
  ctx.setTransform(1, 0, 0, 1, 0, 0);
  ctx.clearRect(0, 0, ctx.canvas.width, ctx.canvas.height);
  const steps = scene.scale ? scaleSwatches(scene.scale.kind, scene.scale.steps, theme) : [];
  const snap = (v: number) => Math.round(v * scale);
  for (let i = 0; i < scene.count; i++) {
    const x0 = snap(scene.x0[i] ?? 0);
    const y0 = snap(scene.y0[i] ?? 0);
    ctx.fillStyle = fillOf(scene, theme, i, steps).css;
    ctx.fillRect(x0, y0, snap(scene.x1[i] ?? 0) - x0, snap(scene.y1[i] ?? 0) - y0);
  }
  ctx.setTransform(scale, 0, 0, scale, 0, 0);
  ctx.font = LABEL_FONT;
  ctx.textBaseline = 'middle';
  for (const label of scene.labels) {
    ctx.fillStyle = fillOf(scene, theme, label.node, steps).inkCss;
    ctx.fillText(label.name, label.x, label.y);
    if (label.value !== null) ctx.fillText(label.value, label.x, label.y + 16);
  }
}

export type Marks = { hover: number | null; selected: number | null };

/** The overlay: the hovered tile outlined, then the selection in the accent. */
export function paintMarks(
  ctx: CanvasRenderingContext2D,
  scene: Scene,
  theme: Theme,
  scale: number,
  marks: Marks,
): void {
  ctx.setTransform(scale, 0, 0, scale, 0, 0);
  ctx.clearRect(0, 0, scene.width, scene.height);
  const outline = (i: number, width: number, style: string) => {
    ctx.lineWidth = width;
    ctx.strokeStyle = style;
    ctx.strokeRect(
      (scene.x0[i] ?? 0) + width / 2,
      (scene.y0[i] ?? 0) + width / 2,
      (scene.x1[i] ?? 0) - (scene.x0[i] ?? 0) - width,
      (scene.y1[i] ?? 0) - (scene.y0[i] ?? 0) - width,
    );
  };
  if (marks.hover !== null) outline(marks.hover, 1, outlineCss(theme));
  if (marks.selected !== null) outline(marks.selected, 2, ACCENT);
}
