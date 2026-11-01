import { hasFilters, type UrlState } from './urlState';

// Every state the page can be in that looks different. The browser walk pictures each one, and its
// table is keyed by this type, so a new state does not compile until it is listed (PRACTICES,
// Testing).
export type ViewState =
  | 'treemap-root'
  | 'treemap-drilled'
  | 'treemap-selected'
  | 'treemap-filtered'
  | 'treemap-generated-shown'
  | 'treemap-colour'
  | 'treemap-ratio-colour'
  | 'treemap-tooltip'
  | 'treemap-empty'
  | 'xtab'
  | 'xtab-by-path'
  | 'xtab-empty'
  | 'source-file'
  | 'source-range'
  | 'area-queue'
  | 'area-expanded'
  | 'area-group-expanded';

export type Facts = { empty: boolean; tooltip: boolean; colourIsRatio: boolean };

/** Names the state a map, cross-tab or source page is in, strongest first; the area queue names
 * its own (it alone knows what kind of area is open). */
export function viewStateOf(state: UrlState, facts: Facts): ViewState {
  if (state.source !== null) return state.lines === null ? 'source-file' : 'source-range';
  if (state.view === 'xtab') {
    if (facts.empty) return 'xtab-empty';
    return state.rows === 'path' || state.cols === 'path' ? 'xtab-by-path' : 'xtab';
  }
  if (facts.empty) return 'treemap-empty';
  if (facts.tooltip) return 'treemap-tooltip';
  if (state.color !== null) return facts.colourIsRatio ? 'treemap-ratio-colour' : 'treemap-colour';
  if (hasFilters(state)) return 'treemap-filtered';
  if (state.includeGenerated) return 'treemap-generated-shown';
  if (state.path !== '') return 'treemap-drilled';
  return state.selected === null ? 'treemap-root' : 'treemap-selected';
}
