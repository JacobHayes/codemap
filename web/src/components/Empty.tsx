import { hasFilters, type Navigate, type UrlState } from '../urlState';

/** What a view says when its filters leave nothing to draw (DESIGN, Views). */
export function Empty({ state, navigate }: { state: UrlState; navigate: Navigate }) {
  return (
    <div className="empty">
      <p>No lines match these filters.</p>
      {hasFilters(state) && (
        <button
          onClick={() => {
            navigate({
              role: null,
              concern: null,
              language: null,
              kind: null,
              includeGenerated: false,
              selected: null,
            });
          }}
        >
          Clear filters
        </button>
      )}
    </div>
  );
}
