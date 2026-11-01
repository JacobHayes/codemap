import { createContext, useContext, useEffect, useState } from 'react';
import type { Refresh } from './generated/Events';

/** The index revision the page last heard of; 0 before the server has said. */
export const RevisionContext = createContext(0);
export const useRevision = () => useContext(RevisionContext);

/** Follows the server's refresh events: the revision changes after each completed re-index, and
 * the queries that read it ask again. */
export function useServerRevision(): number {
  const [revision, setRevision] = useState(0);
  useEffect(() => {
    const events = new EventSource('api/events');
    events.addEventListener('refresh', (e) => {
      setRevision((JSON.parse((e as MessageEvent<string>).data) as Refresh).revision);
    });
    return () => {
      events.close();
    };
  }, []);
  return revision;
}
