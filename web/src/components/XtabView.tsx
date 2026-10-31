import { useState } from 'react';
import type { Catalog, Hidden, Xtab } from '../generated/Answer';
import { counted, exactAmount } from '../format';
import { neutralSwatch, scaleSwatches } from '../palette';
import { filterOf } from '../query';
import { useAnswer } from '../useAnswer';
import { useReportHidden } from '../useReportHidden';
import { useTheme } from '../useTheme';
import type { Navigate, UrlState } from '../urlState';
import { viewStateOf } from '../viewState';
import { Empty } from './Empty';
import { Legend } from './Legend';
import { TooltipBox } from './Tooltip';
import { XTAB_COLS_MAX, XTAB_ROWS_MAX, XtabGrid, type CellPoint } from './XtabGrid';

type XtabProps = {
  state: UrlState;
  navigate: Navigate;
  catalog: Catalog | null;
  /** Tells the filter bar what the answer left out. */
  onHidden: (hidden: Hidden | null) => void;
};

/** The path components a path axis keeps: one below the drilled folder. */
const pathDepthOf = (path: string) => (path === '' ? 1 : path.split('/').length + 1);

const parentOf = (path: string) => path.split('/').slice(0, -1).join('/');

/** What the grid leaves undrawn, in words; empty when it drew everything. */
function undrawn(xtab: Xtab): string {
  const rows = Math.max(0, xtab.rows.length - XTAB_ROWS_MAX);
  const cols = Math.max(0, xtab.cols.length - XTAB_COLS_MAX);
  if (rows + cols === 0) return '';
  const parts = [
    rows > 0 && `${String(rows)} smaller rows`,
    cols > 0 && `${String(cols)} smaller columns`,
  ].filter(Boolean);
  return ` ${parts.join(' and ')} are not drawn; filter to see them.`;
}

type BodyProps = { xtab: Xtab } & Pick<XtabProps, 'state' | 'navigate' | 'catalog'>;

function Body({ xtab, state, navigate, catalog }: BodyProps) {
  const theme = useTheme();
  const [pointed, setPointed] = useState<CellPoint | null>(null);
  const swatches = xtab.scale ? scaleSwatches(xtab.scale.steps, theme) : [];
  return (
    <>
      <div className="xtab-body">
        <XtabGrid
          xtab={xtab}
          swatches={swatches}
          neutral={neutralSwatch('leaf', theme)}
          navigate={navigate}
          onPoint={setPointed}
          onUp={() => {
            if (state.path !== '') navigate({ path: parentOf(state.path), selected: null });
          }}
        />
      </div>
      {pointed && (
        <TooltipBox
          x={pointed.x}
          y={pointed.y}
          title={`${pointed.row} and ${pointed.col}`}
          lines={[exactAmount(pointed.value, xtab.measure.unit)]}
          hint="Click to show these on the map"
          onDismiss={() => {
            setPointed(null);
          }}
        />
      )}
      <footer className="map-foot">
        <p className="selection muted">
          {`${exactAmount(xtab.total, xtab.measure.unit)} across ${counted(xtab.rows.length, 'row')} and ${counted(xtab.cols.length, 'column')}.${undrawn(xtab)}`}
        </p>
        {xtab.scale && <Legend scale={xtab.scale} catalog={catalog} />}
      </footer>
    </>
  );
}

/** The cross-tab: two dimensions crossed, cells on the measure scale, totals in the margins. */
export function XtabView({ state, navigate, catalog, onHidden }: XtabProps) {
  const load = useAnswer(
    {
      kind: 'xtab',
      rows: state.rows,
      cols: state.cols,
      measure: state.size,
      filter: filterOf(state),
      pathDepth: pathDepthOf(state.path),
    },
    'xtab',
  );
  const hidden = load.state === 'ready' ? load.answer.hidden : null;
  useReportHidden(hidden, onHidden);
  if (load.state !== 'ready') {
    return (
      <section className="xtab" data-state={load.state}>
        <p className="muted notice">
          {load.state === 'loading' ? 'Loading' : 'The server did not answer'}
        </p>
      </section>
    );
  }
  const xtab = load.answer;
  const empty = xtab.rows.length === 0;
  const facts = { empty, tooltip: false, colourIsRatio: false };
  return (
    <section className="xtab" data-state="ready" data-view={viewStateOf(state, facts)}>
      {empty ? (
        <div className="xtab-body">
          <Empty state={state} navigate={navigate} />
        </div>
      ) : (
        <Body xtab={xtab} state={state} navigate={navigate} catalog={catalog} />
      )}
    </section>
  );
}
