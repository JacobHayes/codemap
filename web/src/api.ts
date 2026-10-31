import type { Answer } from './generated/Answer';
import type { Query } from './generated/Query';

/** Asks the server one question; the wire types are generated from the Rust ones. */
export async function ask(query: Query): Promise<Answer> {
  const response = await fetch('/api/query', {
    method: 'POST',
    headers: { 'Content-Type': 'application/json' },
    body: JSON.stringify(query),
  });
  if (!response.ok) {
    throw new Error(`query failed: ${String(response.status)}`);
  }
  return (await response.json()) as Answer;
}
