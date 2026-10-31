import { useEffect } from 'react';
import type { Hidden } from './generated/Answer';

/** Tells the filter bar what the view's answer left out, and clears it when the view goes. */
export function useReportHidden(
  hidden: Hidden | null,
  onHidden: (hidden: Hidden | null) => void,
): void {
  useEffect(() => {
    onHidden(hidden);
    return () => {
      onHidden(null);
    };
  }, [hidden, onHidden]);
}
