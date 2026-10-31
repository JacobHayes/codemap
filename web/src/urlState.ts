import { useCallback, useMemo, useSyncExternalStore } from 'react';

// The query string is the only state store (ARCHITECTURE, Web UI): the view, the drill path, the
// filters, the measures and the selection. Only values that differ from the defaults are written,
// so the stock view is the bare address.

type View = 'map' | 'areas';

export type UrlState = {
  view: View;
  /** The drill path: the folder the map shows, relative to the root. Empty for the whole codebase. */
  path: string;
  role: string | null;
  concern: string | null;
  language: string | null;
  /** Whether generated and vendored lines count; the server leaves them out otherwise. */
  includeGenerated: boolean;
  /** The size measure's catalog name. */
  size: string;
  /** The colour measure's catalog name; null for no colour. */
  color: string | null;
  /** The selected tile's path. */
  selected: string | null;
  /** The area unfolded in the queue, by subject. */
  area: string | null;
};

export const DEFAULT_STATE: UrlState = {
  view: 'map',
  path: '',
  role: null,
  concern: null,
  language: null,
  includeGenerated: false,
  size: 'lines',
  color: null,
  selected: null,
  area: null,
};

export const FILTER_DIMENSIONS = ['role', 'concern', 'language'] as const;
export type FilterDimension = (typeof FILTER_DIMENSIONS)[number];

// Query-string keys, one per field.
const KEYS: Record<keyof UrlState, string> = {
  view: 'view',
  path: 'path',
  role: 'role',
  concern: 'concern',
  language: 'language',
  includeGenerated: 'include-generated',
  size: 'size',
  color: 'color',
  selected: 'sel',
  area: 'area',
};

export function parseState(search: string): UrlState {
  const params = new URLSearchParams(search);
  const text = (field: keyof UrlState) => params.get(KEYS[field]);
  return {
    view: text('view') === 'areas' ? 'areas' : 'map',
    path: (text('path') ?? '').replace(/^\/+|\/+$/g, ''),
    role: text('role'),
    concern: text('concern'),
    language: text('language'),
    includeGenerated: text('includeGenerated') === 'true',
    size: text('size') ?? DEFAULT_STATE.size,
    color: text('color'),
    selected: text('selected'),
    area: text('area'),
  };
}

// Slashes stay readable in a path.
const encode = (value: string) => encodeURIComponent(value).replaceAll('%2F', '/');

/** The query string for a state, `?` included; empty for the defaults. */
export function serializeState(state: UrlState): string {
  const parts = (Object.keys(KEYS) as (keyof UrlState)[])
    .filter((field) => state[field] !== DEFAULT_STATE[field])
    .map((field) => `${KEYS[field]}=${encode(String(state[field]))}`);
  return parts.length === 0 ? '' : `?${parts.join('&')}`;
}

export const hasFilters = (state: UrlState) =>
  FILTER_DIMENSIONS.some((dimension) => state[dimension] !== null);

const CHANGED = 'codemap:urlchange';

function subscribe(notify: () => void): () => void {
  window.addEventListener('popstate', notify);
  window.addEventListener(CHANGED, notify);
  return () => {
    window.removeEventListener('popstate', notify);
    window.removeEventListener(CHANGED, notify);
  };
}

export type Navigate = (patch: Partial<UrlState>, mode?: 'push' | 'replace') => void;

/** The state in the address, and a function that changes it. A change is a history entry unless
 * it is only a selection moving, which replaces the current one. */
export function useUrlState(): [UrlState, Navigate] {
  const search = useSyncExternalStore(subscribe, () => window.location.search);
  const state = useMemo(() => parseState(search), [search]);
  const navigate = useCallback<Navigate>(
    (patch, mode = 'push') => {
      const next = serializeState({ ...state, ...patch });
      const url = `${window.location.pathname}${next}`;
      if (next === search) return;
      if (mode === 'push') window.history.pushState(null, '', url);
      else window.history.replaceState(null, '', url);
      window.dispatchEvent(new Event(CHANGED));
    },
    [state, search],
  );
  return [state, navigate];
}
