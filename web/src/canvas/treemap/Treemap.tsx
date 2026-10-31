import { useEffect, useMemo, useRef, useState } from 'react';
import type { Theme } from '../../palette';
import type { Tree } from '../../generated/Answer';
import { layout, LABEL_FONT, type Scene } from './layout';
import { neighbour, type Direction } from './nav';
import { paint, paintMarks } from './paint';
import { pick } from './pick';

/** Where the pointer, or the keyboard's selection, is pointing: a node, and a spot on the page. */
export type Pointed = { nodeId: number; x: number; y: number };

export type TreemapProps = {
  tree: Tree;
  selectedId: number | null;
  onPoint: (pointed: Pointed | null) => void;
  onSelect: (nodeId: number | null) => void;
  onDrill: (nodeId: number) => void;
  onUp: () => void;
};

const ARROWS: Record<string, Direction> = {
  ArrowLeft: 'left',
  ArrowRight: 'right',
  ArrowUp: 'up',
  ArrowDown: 'down',
};

const dark = () => window.matchMedia('(prefers-color-scheme: dark)');

// The theme follows the document: an explicit data-theme wins over the OS.
function currentTheme(): Theme {
  const set = document.documentElement.dataset['theme'];
  if (set === 'light' || set === 'dark') return set;
  return dark().matches ? 'dark' : 'light';
}

function useTheme(): Theme {
  const [theme, setTheme] = useState(currentTheme);
  useEffect(() => {
    const update = () => {
      setTheme(currentTheme());
    };
    const media = dark();
    media.addEventListener('change', update);
    const watcher = new MutationObserver(update);
    watcher.observe(document.documentElement, {
      attributes: true,
      attributeFilter: ['data-theme'],
    });
    return () => {
      media.removeEventListener('change', update);
      watcher.disconnect();
    };
  }, []);
  return theme;
}

function useSize(ref: React.RefObject<HTMLDivElement | null>): { width: number; height: number } {
  const [size, setSize] = useState({ width: 0, height: 0 });
  useEffect(() => {
    const el = ref.current;
    if (!el) return;
    const watcher = new ResizeObserver(([entry]) => {
      if (entry) setSize({ width: entry.contentRect.width, height: entry.contentRect.height });
    });
    watcher.observe(el);
    return () => {
      watcher.disconnect();
    };
  }, [ref]);
  return size;
}

const deviceSize = (width: number, height: number, scale: number) => ({
  width: Math.max(1, Math.round(width * scale)),
  height: Math.max(1, Math.round(height * scale)),
});

// Canvas text falls back to another face until the mono face has loaded; paint again after.
function useFontsReady(): boolean {
  const [ready, setReady] = useState(false);
  useEffect(() => {
    void document.fonts.load(LABEL_FONT).then(() => {
      setReady(true);
    });
  }, []);
  return ready;
}

type KeyActions = Pick<TreemapProps, 'onPoint' | 'onSelect' | 'onDrill' | 'onUp'> & {
  setHover: (i: number | null) => void;
};

// Arrows move the selection among siblings and point at the new tile from its lower left corner.
function useKeys(
  box: React.RefObject<HTMLDivElement | null>,
  scene: Scene,
  selected: number | null,
  act: KeyActions,
) {
  return (e: React.KeyboardEvent) => {
    const direction = ARROWS[e.key];
    if (direction) {
      const next = neighbour(scene, selected, direction);
      if (next === null) {
        e.preventDefault();
        return;
      }
      const r = box.current?.getBoundingClientRect();
      act.onSelect(scene.nodeId[next] ?? null);
      act.setHover(next);
      act.onPoint({
        nodeId: scene.nodeId[next] ?? -1,
        x: (r?.left ?? 0) + (scene.x0[next] ?? 0) + 8,
        y: (r?.top ?? 0) + (scene.y1[next] ?? 0) - 8,
      });
    } else if (e.key === 'Enter' && selected !== null && scene.canDrill[selected]) {
      act.onDrill(scene.nodeId[selected] ?? -1);
    } else if (e.key === 'Backspace') {
      act.onUp();
    } else if (e.key === 'Escape') {
      act.setHover(null);
      act.onPoint(null);
    } else return;
    e.preventDefault();
  };
}

/** The treemap: a picture canvas under an overlay canvas for the outlines. React only hosts it. */
export function Treemap({ tree, selectedId, onPoint, onSelect, onDrill, onUp }: TreemapProps) {
  const box = useRef<HTMLDivElement>(null);
  const picture = useRef<HTMLCanvasElement>(null);
  const overlay = useRef<HTMLCanvasElement>(null);
  const [hover, setHover] = useState<number | null>(null);
  const { width, height } = useSize(box);
  const theme = useTheme();
  const fonts = useFontsReady();
  const scale = window.devicePixelRatio;
  const scene = useMemo<Scene>(() => layout(tree, width, height), [tree, width, height]);
  const selected = useMemo(() => {
    const index = scene.nodeId.indexOf(selectedId ?? -2);
    return index < 1 ? null : index;
  }, [scene, selectedId]);
  const pixels = deviceSize(width, height, scale);

  useEffect(() => {
    const ctx = picture.current?.getContext('2d');
    if (ctx) paint(ctx, scene, theme, scale);
  }, [scene, theme, scale, fonts]);
  useEffect(() => {
    const ctx = overlay.current?.getContext('2d');
    if (ctx) paintMarks(ctx, scene, theme, scale, { hover, selected });
  }, [scene, theme, scale, hover, selected]);

  const at = (e: React.MouseEvent) => {
    const r = e.currentTarget.getBoundingClientRect();
    return pick(scene, e.clientX - r.left, e.clientY - r.top);
  };
  const onKeyDown = useKeys(box, scene, selected, { setHover, onPoint, onSelect, onDrill, onUp });
  return (
    <div
      ref={box}
      className="treemap"
      role="group"
      aria-label="Treemap. Arrow keys move between tiles, Enter opens a folder, Backspace goes up, Escape closes the tooltip."
      tabIndex={0}
      style={{ position: 'relative', width: '100%', height: '100%' }}
      data-tiles={scene.count}
      onKeyDown={onKeyDown}
    >
      <canvas ref={picture} {...pixels} style={{ position: 'absolute', width, height }} />
      <canvas
        ref={overlay}
        {...pixels}
        style={{ position: 'absolute', width, height }}
        onMouseMove={(e) => {
          const i = at(e);
          setHover(i);
          onPoint(
            i === null ? null : { nodeId: scene.nodeId[i] ?? -1, x: e.clientX, y: e.clientY },
          );
        }}
        onMouseLeave={() => {
          setHover(null);
          onPoint(null);
        }}
        onClick={(e) => {
          const i = at(e);
          box.current?.focus();
          onSelect(i === null || i === 0 ? null : (scene.nodeId[i] ?? null));
        }}
        onDoubleClick={(e) => {
          const i = at(e);
          if (i !== null && scene.canDrill[i]) onDrill(scene.nodeId[i] ?? -1);
        }}
      />
    </div>
  );
}
