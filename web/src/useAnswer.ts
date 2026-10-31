import { useEffect, useState } from 'react';
import { ask } from './api';
import type { Envelope } from './generated/Answer';
import type { Query } from './generated/Query';
import { useRevision } from './revision';

export type Load<T> = { state: 'loading' } | { state: 'failed' } | { state: 'ready'; answer: T };

type Settled = { key: string; load: Load<Envelope> };

/** The server's answer to a query, or `loading`. The answer is keyed by its query: while the query
 * changes, an answer to an earlier one is never shown. A new index revision asks again and swaps
 * the answer in place. A null query asks nothing. */
export function useAnswer<K extends Envelope['kind']>(
  query: Query | null,
  kind: K,
): Load<Extract<Envelope, { kind: K }>> {
  const key = query === null ? null : JSON.stringify(query);
  const revision = useRevision();
  const [settled, setSettled] = useState<Settled | null>(null);
  useEffect(() => {
    if (query === null || key === null) return;
    let current = true;
    ask(query).then(
      (answer) => {
        if (current) setSettled({ key, load: { state: 'ready', answer } });
      },
      () => {
        if (current) setSettled({ key, load: { state: 'failed' } });
      },
    );
    return () => {
      current = false;
    };
  }, [key, revision]);
  if (settled?.key !== key) return { state: 'loading' };
  const { load } = settled;
  if (load.state !== 'ready') return load;
  return load.answer.kind === kind
    ? { state: 'ready', answer: load.answer as Extract<Envelope, { kind: K }> }
    : { state: 'failed' };
}
