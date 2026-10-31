import { useEffect, useRef } from 'react';
import type { Source, SourceFinding, SourceLine } from '../generated/Answer';
import { exactAmount, words } from '../format';
import { EARLIER_LINES, formatLines, parseLines } from '../source';
import { useAnswer } from '../useAnswer';
import type { Navigate, UrlState } from '../urlState';
import { Link } from './Link';

type Common = { state: UrlState; navigate: Navigate };

/** The source query for a state: the lines in the address, or the start of the file. */
function sourceQuery(path: string, lines: string | null) {
  const range = parseLines(lines);
  return {
    kind: 'source' as const,
    path,
    ...(range ? { startLine: range.start } : {}),
    ...(range?.end != null ? { endLine: range.end } : {}),
  };
}

/** Whether a finding names the whole file, which is listed and not painted line by line. */
const wholeFile = (finding: SourceFinding, source: Source) =>
  finding.startLine <= 1 && finding.endLine >= source.lineCount;

/** What the gutter says about a line: the role and concern, where they change. */
function marks(line: SourceLine, before: SourceLine | undefined): string {
  const parts = [
    line.role !== before?.role && line.role,
    line.concern !== before?.concern && line.concern,
  ];
  return parts.filter(Boolean).join(' ');
}

/** Back to the area a finding belongs to: the drill path is cleared so the queue lists it. */
const areaPatch = (f: SourceFinding): Partial<UrlState> => ({
  view: 'areas',
  area: f.area,
  path: '',
  source: null,
  lines: null,
});

function Findings({ source, state, navigate }: { source: Source } & Common) {
  if (source.findings.length === 0) return null;
  return (
    <details className="source-findings">
      <summary>{`${String(source.findings.length)} findings touch these lines`}</summary>
      <ul>
        {source.findings.map((f) => (
          <li key={`${f.detector}:${String(f.startLine)}:${f.area}`}>
            <Link state={state} patch={areaPatch(f)} navigate={navigate}>
              {words(f.detector)}
            </Link>
            {` ${f.reason} Lines ${String(f.startLine)} to ${String(f.endLine)}.`}
          </li>
        ))}
      </ul>
    </details>
  );
}

type RowProps = { line: SourceLine; before: SourceLine | undefined; source: Source } & Common;

function Row({ line, before, source, state, navigate }: RowProps) {
  const n = line.number;
  const starting = source.findings.filter(
    (f) => f.startLine === n || (n === source.startLine && f.startLine < n),
  );
  const flagged = source.findings.some(
    (f) => f.startLine <= n && f.endLine >= n && !wholeFile(f, source),
  );
  return (
    <tr className={flagged ? 'flagged' : undefined}>
      <td className="gutter num">{n}</td>
      <td className="marks">{marks(line, before)}</td>
      <td className="text">{line.text === '' ? ' ' : line.text}</td>
      <td className="links">
        {starting.map((f) => (
          <Link
            key={`${f.detector}:${f.area}`}
            state={state}
            patch={areaPatch(f)}
            navigate={navigate}
          >
            {words(f.detector)}
          </Link>
        ))}
      </td>
    </tr>
  );
}

function Paging({ source, state, navigate }: { source: Source } & Common) {
  const { startLine: start, endLine: end } = source;
  const to = (range: string | null) => ({ lines: range });
  return (
    <footer className="map-foot">
      <p className="selection muted">
        {`Lines ${String(start)} to ${String(end)} of ${String(source.lineCount)}.`}
        {source.more > 0 && ` ${exactAmount(source.more, 'lines')} follow.`}
      </p>
      <div className="paging">
        {start > 1 && (
          <>
            <Link state={state} patch={to(null)} navigate={navigate} className="button">
              From the top
            </Link>
            <Link
              state={state}
              patch={to(formatLines({ start: Math.max(1, start - EARLIER_LINES), end: start - 1 }))}
              navigate={navigate}
              className="button"
            >
              Earlier lines
            </Link>
          </>
        )}
        {source.more > 0 && (
          <Link
            state={state}
            patch={to(formatLines({ start: end + 1, end: null }))}
            navigate={navigate}
            className="button"
          >
            Next lines
          </Link>
        )}
      </div>
    </footer>
  );
}

function Lines({ source, state, navigate }: { source: Source } & Common) {
  const body = useRef<HTMLDivElement>(null);
  // Evidence opens with its surroundings; bring the first flagged line into view.
  useEffect(() => {
    if (state.lines !== null) {
      body.current?.querySelector('tr.flagged')?.scrollIntoView({ block: 'center' });
    }
  }, [source, state.lines]);
  return (
    <div className="source-body" role="region" aria-label="Source lines" tabIndex={0} ref={body}>
      <Findings source={source} state={state} navigate={navigate} />
      <table className="code">
        <caption className="visually-hidden">{`Lines of ${source.path}`}</caption>
        <tbody>
          {source.lines.map((line, i) => (
            <Row
              key={line.number}
              line={line}
              before={source.lines[i - 1]}
              source={source}
              state={state}
              navigate={navigate}
            />
          ))}
        </tbody>
      </table>
    </div>
  );
}

/** One file's lines with a gutter; finding ranges are highlighted and link to their area. */
export function SourceView({ state, navigate, path }: Common & { path: string }) {
  const load = useAnswer(sourceQuery(path, state.lines), 'source');
  const close = () => {
    navigate({ source: null, lines: null });
  };
  if (load.state !== 'ready') {
    return (
      <section className="source" data-state={load.state}>
        <p className="muted notice">
          {load.state === 'loading'
            ? 'Loading'
            : `${path} cannot be shown: it may be binary, changed since the last index, or not in the index.`}
        </p>
        {load.state === 'failed' && (
          <p className="notice">
            <button onClick={close}>Close</button>
          </p>
        )}
      </section>
    );
  }
  const source = load.answer;
  return (
    <section
      className="source"
      data-state="ready"
      data-view={state.lines === null ? 'source-file' : 'source-range'}
    >
      <header className="source-head">
        <span className="mono source-path">{source.path}</span>
        <span className="muted">
          {`${source.language ?? 'No grammar'}, ${exactAmount(source.lineCount, 'lines')}`}
        </span>
        <button onClick={close}>Close</button>
      </header>
      <Lines source={source} state={state} navigate={navigate} />
      <Paging source={source} state={state} navigate={navigate} />
    </section>
  );
}
