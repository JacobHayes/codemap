import { useEffect, useMemo, useRef, useState } from 'react';
import type { Theme } from '../../palette';
import { layout, type Scene, type TreeInput } from './layout';
import { paint, paintMarks } from './paint';
import { pathOf, pick } from './pick';

export type TreemapProps = {
  input: TreeInput;
  drillRootId: number | null;
  selectedId: number | null;
  onHover: (hover: { nodeId: number; path: string } | null) => void;
  onSelect: (nodeId: number) => void;
  onDrill: (nodeId: number) => void;
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

/** The treemap: a picture canvas under an overlay canvas for the outlines. React only hosts it. */
export function Treemap({
  input,
  drillRootId,
  selectedId,
  onHover,
  onSelect,
  onDrill,
}: TreemapProps) {
  const box = useRef<HTMLDivElement>(null);
  const picture = useRef<HTMLCanvasElement>(null);
  const overlay = useRef<HTMLCanvasElement>(null);
  const [hover, setHover] = useState<number | null>(null);
  const { width, height } = useSize(box);
  const theme = useTheme();
  const scale = window.devicePixelRatio;
  const scene = useMemo<Scene>(
    () => layout(input, width, height, drillRootId),
    [input, width, height, drillRootId],
  );
  const selected = useMemo(() => {
    const index = scene.nodeId.indexOf(selectedId ?? -2);
    return index < 0 ? null : index;
  }, [scene, selectedId]);
  const pixels = deviceSize(width, height, scale);

  useEffect(() => {
    const ctx = picture.current?.getContext('2d');
    if (ctx) paint(ctx, scene, theme, scale);
  }, [scene, theme, scale]);
  useEffect(() => {
    const ctx = overlay.current?.getContext('2d');
    if (ctx) paintMarks(ctx, scene, theme, scale, { hover, selected });
  }, [scene, theme, scale, hover, selected]);

  const at = (e: React.MouseEvent) => {
    const r = e.currentTarget.getBoundingClientRect();
    return pick(scene, e.clientX - r.left, e.clientY - r.top);
  };
  return (
    <div
      ref={box}
      className="treemap"
      style={{ position: 'relative', width: '100%', height: '100%' }}
    >
      <canvas ref={picture} {...pixels} style={{ position: 'absolute', width, height }} />
      <canvas
        ref={overlay}
        {...pixels}
        style={{ position: 'absolute', width, height }}
        onMouseMove={(e) => {
          const i = at(e);
          setHover(i);
          onHover(i === null ? null : { nodeId: scene.nodeId[i] ?? -1, path: pathOf(scene, i) });
        }}
        onMouseLeave={() => {
          setHover(null);
          onHover(null);
        }}
        onClick={(e) => {
          const i = at(e);
          if (i !== null) onSelect(scene.nodeId[i] ?? -1);
        }}
        onDoubleClick={(e) => {
          const i = at(e);
          if (i !== null && scene.canDrill[i]) onDrill(scene.nodeId[i] ?? -1);
        }}
      />
    </div>
  );
}
