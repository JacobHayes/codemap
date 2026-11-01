import type { Area, AreaDetail, AreaList, Finding } from '../generated/Answer';
import { amount, percentOf, words } from '../format';
import { around, formatLines } from '../source';
import { useAnswer } from '../useAnswer';
import { filterOf } from '../query';
import { hasFilters, type Navigate, type UrlState } from '../urlState';
import type { ViewState } from '../viewState';
import { Link } from './Link';

function viewOf(list: AreaList, state: UrlState): ViewState {
  if (state.area === null) return 'area-queue';
  return state.area.startsWith('group:') ? 'area-group-expanded' : 'area-expanded';
}

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
}: { finding: Finding; area: Area | null } & Common) {
  return (
    <li className="finding">
      <div>
        <span className="label">{words(finding.detector)}</span>
        {finding.reason !== area?.reason && ` ${finding.reason}`}
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
                patch={{
                  source: e.path,
                  lines: formatLines(around(e.startLine, e.endLine)),
                }}
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
      {detail.groups.length > 0 && (
        <>
          <h2>Groups this belongs to</h2>
          <ul className="related">
            {detail.groups.map((share) => (
              <li key={share.group.subject}>
                <Link state={state} patch={{ area: share.group.subject }} navigate={navigate}>
                  {share.group.title}
                </Link>
                {`: ${amount(share.lines, 'lines')} of it here, ${percentOf(share.share)} of the group`}
              </li>
            ))}
          </ul>
        </>
      )}
      {detail.related.length > 0 && (
        <>
          <h2>{detail.area?.subjectKind === 'group' ? 'Areas of its files' : 'Nearby areas'}</h2>
          <ul className="related mono">
            {detail.related.map((area) => (
              <li key={area.subject}>
                <Link state={state} patch={{ area: area.subject }} navigate={navigate}>
                  {area.title}
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
        {area.subjectKind === 'group' ? (
          <span className="area-title">{area.title}</span>
        ) : (
          <Link state={state} patch={mapPatch(area)} navigate={navigate} className="area-path mono">
            {area.title}
          </Link>
        )}
        <span className="area-mass">
          {amount(area.massLines, 'lines')}
          {area.subjectKind === 'file' || area.subjectKind === 'function' ? (
            <>
              {' '}
              <Link
                state={state}
                patch={{ source: area.subject.split('::')[0] ?? area.subject, lines: null }}
                navigate={navigate}
              >
                Source
              </Link>
            </>
          ) : null}
        </span>
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
          `${String(hidden)} more ${hidden === 1 ? 'area is' : 'areas are'} hidden by the drill path or filters. `}
        Select a path to show it on the map.
      </p>
      {semantic !== '' && <p className="muted">{semantic}</p>}
    </div>
  );
}

/** The ranked areas: each card says where, why, who agrees and how much; evidence folds out. */
export function AreaQueue({ state, navigate }: Common) {
  // Generated and vendored lines count only when a role names them, so that flag stays off.
  const areas = useAnswer(
    { kind: 'areas', filter: { ...filterOf(state), includeGenerated: false } },
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
    <section className="areas" data-state="ready" data-view={viewOf(list, state)}>
      <Summary list={list} total={total} state={state} semantic={semantic} />
      {state.area !== null && !list.areas.some((a) => a.subject === state.area) && (
        // Reached from a link whose target the filters leave out of the list.
        <div className="card area">
          <div className="card-body">
            <Detail subject={state.area} state={state} navigate={navigate} />
          </div>
        </div>
      )}
      {list.areas.length === 0 ? (
        <div className="empty">
          <p>No areas stand out here.</p>
          {hasFilters(state) && (
            <button
              onClick={() => {
                navigate({ role: null, concern: null, language: null, kind: null, area: null });
              }}
            >
              Clear filters
            </button>
          )}
        </div>
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
