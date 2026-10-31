import type { Catalog, Scale } from '../generated/Answer';
import { compact, plain } from '../format';
import { neutralSwatch, scaleSwatches } from '../palette';
import { useTheme } from '../useTheme';

type LegendProps = { scale: Scale; catalog: Catalog | null };

/** The colour scale in words and numbers: the measure and its range. */
export function Legend({ scale, catalog }: LegendProps) {
  const theme = useTheme();
  const def = catalog?.measures.find((m) => m.name === scale.measure);
  const isRatio = def?.measure.kind === 'ratio';
  const number = isRatio ? plain : compact;
  const swatches = scaleSwatches(scale.steps, theme);
  return (
    <figure className="legend" aria-label="Colour scale">
      <figcaption>
        <span className="label">Colour</span> {def?.description ?? 'Colour measure'}
      </figcaption>
      <div className="legend-bar" aria-hidden="true">
        {swatches.map((swatch, i) => (
          <span key={i} style={{ background: swatch.css }} />
        ))}
      </div>
      <div className="legend-range">
        <span>{number(scale.min)}</span>
        <span>{number(scale.max)}</span>
      </div>
      <p className="legend-note">
        <span
          className="legend-none"
          style={{ background: neutralSwatch('leaf', theme).css }}
          aria-hidden="true"
        />
        {isRatio ? 'Zero or no value' : 'No value'}
        <span className="legend-steps">
          {scale.spacing === 'log' ? 'Steps grow by multiples.' : 'Steps are equal.'}
        </span>
      </p>
    </figure>
  );
}
