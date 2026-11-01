import AxeBuilder from '@axe-core/playwright';
import { expect, type APIRequestContext, type Page } from '@playwright/test';
import type { Envelope } from '../src/generated/Answer';
import type { Query } from '../src/generated/Query';

// Properties that hold on every page (PRACTICES, Testing): checked on every page of every browser
// test, so a new view cannot break them unnoticed.

async function ask(request: APIRequestContext, query: Query): Promise<Envelope> {
  const response = await request.post('/api/query', { data: query });
  if (!response.ok()) throw new Error(`${query.kind}: ${await response.text()}`);
  return (await response.json()) as Envelope;
}

const isInternal = (name: string) => /_|[a-z][A-Z]/.test(name);

/** Names the server uses that are written in code style (snake_case or camelCase): the catalog's
 * and what findings carry. They must never be read off the screen. */
async function internalIds(request: APIRequestContext): Promise<string[]> {
  const ids = new Set<string>(['codemap.answer']);
  const catalog = await ask(request, { kind: 'catalog' });
  if (catalog.kind === 'catalog') {
    for (const m of catalog.measures) ids.add(m.name);
    for (const d of catalog.detectors) ids.add(d.name);
    for (const d of catalog.dimensions) ids.add(d.name);
  }
  const areas = await ask(request, { kind: 'areas', filter: { includeGenerated: false } });
  if (areas.kind === 'areas') {
    for (const area of areas.areas) {
      const detail = await ask(request, { kind: 'area', subject: area.subject });
      if (detail.kind !== 'area') continue;
      for (const finding of detail.findings) {
        Object.keys(finding.measures).forEach((k) => ids.add(k));
        Object.keys(finding.labels).forEach((k) => ids.add(k));
      }
    }
  }
  return [...ids].filter(isInternal);
}

/** Everything wrong with the page as drawn, in words; empty when it is sound. */
function problemsOnPage(ids: string[]): string[] {
  const problems: string[] = [];
  const name = (el: Element) =>
    `<${el.tagName.toLowerCase()} class="${el.getAttribute('class') ?? ''}"> "${el.textContent.trim().slice(0, 40)}"`;
  const root = document.documentElement;
  if (root.scrollHeight > innerHeight || root.scrollWidth > innerWidth) {
    problems.push('the page scrolls');
  }
  for (const el of document.body.querySelectorAll('*')) {
    if (el.closest('canvas, svg, .visually-hidden, select')) continue;
    const style = getComputedStyle(el);
    if (style.display === 'none' || style.display === 'contents' || style.visibility === 'hidden') {
      continue;
    }
    const text = el.textContent.trim() !== '';
    const scrolls = style.overflowX === 'auto' || style.overflowX === 'scroll';
    const ownText = [...el.childNodes].some(
      (n) => n.nodeType === 3 && (n.textContent ?? '').trim(),
    );
    if (ownText && parseFloat(style.fontSize) < 12) problems.push(`${name(el)} is under 12px`);
    if (text && style.display !== 'inline' && el.scrollWidth > el.clientWidth + 1 && !scrolls) {
      if (style.overflowX === 'visible') problems.push(`${name(el)} spills out sideways`);
      else if (style.textOverflow !== 'ellipsis')
        problems.push(`${name(el)} cuts text without an ellipsis`);
    }
    if (text && style.display !== 'inline' && el.scrollHeight > el.clientHeight + 1) {
      const clips = style.overflowY === 'hidden' || style.overflowY === 'clip';
      if (clips) problems.push(`${name(el)} cuts text vertically`);
    }
    const parent = el.parentElement;
    if (parent && style.position !== 'fixed' && style.position !== 'absolute') {
      const parentStyle = getComputedStyle(parent);
      const box = el.getBoundingClientRect();
      const around = parent.getBoundingClientRect();
      const free = parentStyle.overflowX === 'auto' || parentStyle.overflowX === 'scroll';
      if (!free && box.width > 0 && (box.left < around.left - 1 || box.right > around.right + 1)) {
        problems.push(`${name(el)} sticks out of ${name(parent)}`);
      }
    }
  }
  const shown = document.body.innerText;
  for (const id of ids) {
    if (new RegExp(`\\b${id}\\b`).test(shown))
      problems.push(`the internal id "${id}" is on screen`);
  }
  for (const junk of ['undefined', 'NaN', '[object']) {
    if (shown.includes(junk)) problems.push(`"${junk}" is on screen`);
  }
  return problems;
}

/** Fails when the page spills, scrolls, shows an internal id or small text, or breaks axe. */
export async function expectSound(page: Page): Promise<void> {
  await page.evaluate(() => document.fonts.ready);
  const ids = await internalIds(page.request);
  expect(await page.evaluate(problemsOnPage, ids), 'structural problems').toEqual([]);
  const axe = await new AxeBuilder({ page }).analyze();
  expect(
    axe.violations.map((v) => `${v.id}: ${v.nodes.map((n) => n.target.join(' ')).join(', ')}`),
    'accessibility violations',
  ).toEqual([]);
}
