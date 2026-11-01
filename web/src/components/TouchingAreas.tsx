import type { AreaShare } from '../generated/Answer';
import { amount } from '../format';
import { filterOf } from '../query';
import { useAnswer } from '../useAnswer';
import { type Navigate, type UrlState } from '../urlState';
import { Link } from './Link';

/** How many areas a tile lists before saying how many more there are. */
const LISTED = 5;

/** The open areas with evidence in a file or folder, with the lines of each that lie there. A
 * null path asks nothing. */
export function useTouching(path: string | null, state: UrlState): AreaShare[] | null {
  // The map's filters, so the list counts the lines the tile's colour counts. The subject says
  // where, so the drill path is left out.
  const filter = filterOf({ ...state, path: '' });
  const load = useAnswer(path === null ? null : { kind: 'area', subject: path, filter }, 'area');
  return load.state === 'ready' ? load.answer.touching : null;
}

type Props = {
  shares: AreaShare[];
  /** Links to the areas when a state and a way to navigate are given; plain text otherwise. */
  link?: { state: UrlState; navigate: Navigate };
};

/** The areas touching a tile, most lines first. */
export function TouchingAreas({ shares, link }: Props) {
  if (shares.length === 0) return null;
  const shown = shares.slice(0, LISTED);
  return (
    <ul className="touching">
      {shown.map(({ area, lines }) => (
        <li key={area.subject}>
          {link ? (
            <Link
              state={link.state}
              patch={{ view: 'areas', area: area.subject }}
              navigate={link.navigate}
            >
              {area.title}
            </Link>
          ) : (
            <span>{area.title}</span>
          )}
          <span className="touching-lines">{`: ${amount(lines, 'lines')} here`}</span>
        </li>
      ))}
      {shares.length > shown.length && (
        <li className="muted">{`and ${String(shares.length - shown.length)} more`}</li>
      )}
    </ul>
  );
}
