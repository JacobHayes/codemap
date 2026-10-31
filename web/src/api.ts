import type { Envelope } from './generated/Answer';
import type { Query } from './generated/Query';

/** Asks the server one question; the wire types are generated from the Rust ones. */
export async function ask(query: Query): Promise<Envelope> {
  const response = await fetch('/api/query', {
    method: 'POST',
    headers: { 'Content-Type': 'application/json' },
    body: JSON.stringify(query),
  });
  if (!response.ok) {
    throw new Error(`query failed: ${String(response.status)}`);
  }
  return (await response.json()) as Envelope;
}
