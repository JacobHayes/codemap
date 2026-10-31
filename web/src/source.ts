// The lines of a file the source viewer shows: `a-b`, `a` for one line, `a-` for everything from
// `a` on. The server caps what one answer returns, so a page is a range and the next begins after it.

export type LineRange = { start: number; end: number | null };

/** Lines of context around a finding's evidence when it is opened in the viewer. */
const CONTEXT_LINES = 20;

/** Lines the earlier-lines button goes back by. */
export const EARLIER_LINES = 100;

/** The range a `lines` value names; null when it is absent or not a range. */
export function parseLines(text: string | null): LineRange | null {
  const match = /^(\d+)(?:(-)(\d*))?$/.exec(text ?? '');
  const start = Number(match?.[1]);
  if (!match || start < 1) return null;
  if (match[2] === undefined) return { start, end: start };
  return { start, end: match[3] === '' ? null : Number(match[3]) };
}

/** The `lines` value for a range. */
export const formatLines = ({ start, end }: LineRange): string =>
  end === null ? `${String(start)}-` : `${String(start)}-${String(end)}`;

/** The range that shows evidence with its surroundings. */
export const around = (startLine: number, endLine: number): LineRange => ({
  start: Math.max(1, startLine - CONTEXT_LINES),
  end: endLine + CONTEXT_LINES,
});
