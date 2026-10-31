import type { Area, AreaDetail, AreaList, Finding } from '../generated/Answer';
import { amount, words } from '../format';
import { useAnswer } from '../useAnswer';
import { hasFilters, type Navigate, type UrlState } from '../urlState';
import { Link } from './Link';

type Common = { state: UrlState; navigate: Navigate };

const parentOf = (path: string) => path.split('/').slice(0, -1).join('/');

/** Where the map shows an area: a folder is opened, a file is selected in its folder. */
function mapPatch(area: Area): Partial<UrlState> {
  return area.subjectKind === 'directory'
    ? { view: 'map', path: area.subject, selected: null, area: null }
    : { view: 'map', path: parentOf(area.subject), selected: area.subject, area: null };
}

function FindingView({
  finding,
  area,
  state,
  navigate,
}: { finding: Finding; area: Area } & Common) {
  return (
    <li className="finding">
      <div>
        <span className="label">{words(finding.detector)}</span>
        {finding.reason !== area.reason && ` ${finding.reason}`}
      </div>
      {finding.acknowledged !== null && (
        <div className="muted">Acknowledged: {finding.acknowledged}</div>
      )}
      {finding.facts.length > 0 && (
        <ul className="facts">
          {finding.facts.map((fact) => (
            <li key={fact}>{fact}</li>
          ))}
        </ul>
      )}
      {finding.evidence.length > 0 && (
        <ul className="evidence mono">
          {finding.evidence.map((e) => (
            <li key={`${e.path}:${String(e.startLine)}`}>
              <Link
                state={state}
                patch={{ view: 'map', path: parentOf(e.path), selected: e.path, area: null }}
                navigate={navigate}
              >
                {e.path}
              </Link>
              {`, lines ${String(e.startLine)} to ${String(e.endLine)}`}
            </li>
          ))}
        </ul>
      )}
    </li>
  );
}

function Detail({ subject, state, navigate }: { subject: string } & Common) {
  const load = useAnswer({ kind: 'area', subject }, 'area');
  if (load.state !== 'ready') {
    return (
      <p className="muted">{load.state === 'loading' ? 'Loading' : 'The server did not answer'}</p>
    );
  }
  const detail: AreaDetail = load.answer;
  return (
    <div data-state="ready">
      <ul className="findings">
        {detail.findings.map((finding) => (
          <FindingView
            key={`${finding.detector}:${finding.subject}`}
            finding={finding}
            area={detail.area}
            state={state}
            navigate={navigate}
          />
        ))}
      </ul>
      {detail.related.length > 0 && (
        <>
          <h3>Nearby areas</h3>
          <ul className="related mono">
            {detail.related.map((area) => (
              <li key={area.subject}>
                <Link state={state} patch={{ area: area.subject }} navigate={navigate}>
                  {area.subject}
                </Link>
              </li>
            ))}
          </ul>
        </>
      )}
    </div>
  );
}

function AreaCard({ area, state, navigate }: { area: Area } & Common) {
  const open = state.area === area.subject;
  return (
    <li className={`card area${area.acknowledged === null ? '' : ' acknowledged'}`}>
      <div className="card-head">
        <Link state={state} patch={mapPatch(area)} navigate={navigate} className="area-path mono">
          {area.subject}
        </Link>
        <span className="area-mass">{amount(area.massLines, 'lines')}</span>
      </div>
      <div className="card-body">
        <p className="area-reason">{area.reason}</p>
        {area.acknowledged !== null && (
          <p className="muted">Acknowledged as essential: {area.acknowledged}</p>
        )}
        <div className="chips">
          {area.detectors.map((detector) => (
            <span className="badge ink" key={detector}>
              {words(detector)}
            </span>
          ))}
        </div>
        <details
          open={open}
          onToggle={(e) => {
            const now = e.currentTarget.open;
            if (now !== open) navigate({ area: now ? area.subject : null }, 'replace');
          }}
        >
          <summary>Facts and evidence</summary>
          {open && <Detail subject={area.subject} state={state} navigate={navigate} />}
        </details>
      </div>
    </li>
  );
}

function Summary({
  list,
  total,
  state,
  semantic,
}: { list: AreaList; total: number | null; semantic: string } & Pick<Common, 'state'>) {
  const hidden = total === null ? 0 : total - list.matching;
  const where = state.path === '' ? 'in the codebase' : `under ${state.path}`;
  return (
    <div className="queue-summary">
      <p>
        {`${String(list.matching)} ${list.matching === 1 ? 'area' : 'areas'} ${where}`}
        {list.acknowledged > 0 && `, ${String(list.acknowledged)} acknowledged and listed last`}
        {'. '}
        {hidden > 0 &&
          `${String(hidden)} more ${hidden === 1 ? 'area is' : 'areas are'} hidden by the drill path. `}
        {hasFilters(state) && 'The filters apply to the map and cross-tab only. '}
        Select a path to show it on the map.
      </p>
      {semantic !== '' && <p className="muted">{semantic}</p>}
    </div>
  );
}

/** The ranked areas: each card says where, why, who agrees and how much; evidence folds out. */
export function AreaQueue({ state, navigate }: Common) {
  const areas = useAnswer(
    state.path === '' ? { kind: 'areas' } : { kind: 'areas', path: state.path },
    'areas',
  );
  const overview = useAnswer({ kind: 'overview' }, 'overview');
  if (areas.state !== 'ready') {
    return (
      <section className="areas" data-state={areas.state}>
        <p className="muted">
          {areas.state === 'loading' ? 'Loading' : 'The server did not answer'}
        </p>
      </section>
    );
  }
  const list = areas.answer;
  const total = overview.state === 'ready' ? overview.answer.areas.total : null;
  const semantic = overview.state === 'ready' ? overview.answer.semantic : '';
  return (
    <section
      className="areas"
      data-state="ready"
      data-view={state.area === null ? 'area-queue' : 'area-expanded'}
    >
      <Summary list={list} total={total} state={state} semantic={semantic} />
      {list.areas.length === 0 ? (
        <p>No areas stand out here.</p>
      ) : (
        <ol className="area-list">
          {list.areas.map((area) => (
            <AreaCard key={area.subject} area={area} state={state} navigate={navigate} />
          ))}
        </ol>
      )}
    </section>
  );
}
