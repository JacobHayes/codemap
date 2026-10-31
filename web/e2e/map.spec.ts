import { expect, test } from './fixtures';

const map = (page: import('@playwright/test').Page) =>
  page.getByRole('group', { name: /^Treemap/ });

test('the keyboard moves the selection, drills in, goes up and closes the tooltip', async ({
  page,
}) => {
  await page.goto('/?path=crates');
  await expect(page.locator('.treemap')).not.toHaveAttribute('data-tiles', '0');
  await map(page).focus();
  await page.keyboard.press('ArrowRight');
  await expect(page.getByRole('tooltip')).toBeVisible();
  const first = new URL(page.url()).searchParams.get('sel');
  expect(first).not.toBeNull();
  await page.keyboard.press('Escape');
  await expect(page.getByRole('tooltip')).toHaveCount(0);
  await page.keyboard.press('ArrowDown');
  await page.keyboard.press('ArrowRight');
  const moved = new URL(page.url()).searchParams.get('sel');
  expect(moved).not.toBe(first);
  await page.keyboard.press('Backspace');
  await expect.poll(() => new URL(page.url()).searchParams.get('path')).toBeNull();
  await expect(page.getByRole('navigation', { name: 'Drill path' })).toContainText('All files');
});

test('Enter drills into the selected folder and the breadcrumb returns', async ({ page }) => {
  await page.goto('/');
  await expect(page.locator('.treemap')).not.toHaveAttribute('data-tiles', '0');
  await page.goto('/?sel=crates');
  await expect(page.locator('.treemap')).not.toHaveAttribute('data-tiles', '0');
  await map(page).focus();
  await page.keyboard.press('Enter');
  await expect(page).toHaveURL(/path=crates$/);
  const trail = page.getByRole('navigation', { name: 'Drill path' });
  await expect(trail.getByRole('link', { name: 'crates' })).toHaveAttribute(
    'aria-current',
    'location',
  );
  await trail.getByRole('link', { name: 'All files' }).click();
  await expect(page).toHaveURL(/^[^?]*$/);
});

test('a filter is a chip in the address, and removing it restores the stock view', async ({
  page,
}) => {
  await page.goto('/');
  await page.getByLabel('Add a filter').selectOption('role=test');
  await expect(page).toHaveURL(/role=test/);
  await expect(page.getByRole('group', { name: 'Filters' })).toContainText('test');
  await page.getByRole('button', { name: 'Remove filter role test' }).click();
  await expect(page).toHaveURL(/^[^?]*$/);
});

test('a filter that leaves nothing says so and offers to clear it', async ({ page }) => {
  await page.goto('/?role=vendored&language=python');
  await expect(page.getByText('No lines match these filters.')).toBeVisible();
  await page.getByRole('button', { name: 'Clear filters' }).click();
  await expect(page).toHaveURL(/^[^?]*$/);
});

test('a ratio colour measure shows its scale in words and numbers', async ({ page }) => {
  await page.goto('/?color=test_to_prod_ratio');
  const legend = page.getByRole('figure', { name: 'Colour scale' });
  await expect(legend).toContainText('Test lines per line of production code');
  await expect(legend).toContainText('Zero or no value');
});

test('choosing an area in the queue shows it on the map', async ({ page }) => {
  await page.goto('/?view=areas');
  const first = page.locator('.area-path').first();
  const subject = (await first.textContent()) ?? '';
  await first.click();
  await expect(page.locator('.map')).toHaveAttribute('data-state', 'ready');
  const params = new URL(page.url()).searchParams;
  expect(params.get('view')).toBeNull();
  expect(params.get('path') ?? params.get('sel')).not.toBeNull();
  expect(subject.startsWith(params.get('path') ?? '')).toBe(true);
});

test('an area unfolds its evidence and the address remembers it', async ({ page }) => {
  await page.goto('/?view=areas');
  await page.getByText('Facts and evidence').first().click();
  await expect(page).toHaveURL(/area=/);
  await expect(page.locator('.findings')).toBeVisible();
});

test('generated and vendored code is hidden until its chip is removed, and back', async ({
  page,
}) => {
  await page.goto('/');
  await expect(page.locator('.treemap')).not.toHaveAttribute('data-tiles', '0');
  expect(new URL(page.url()).search).toBe('');
  const hiding = page.locator('.filters .chip', { hasText: 'Hiding' });
  await expect(hiding).toBeVisible();
  await hiding.getByRole('button').click();
  await expect(page).toHaveURL(/include-generated=true/);
  await expect(hiding).toHaveCount(0);
  const including = page.locator('.filters .chip', { hasText: 'Including' });
  await including.getByRole('button').click();
  await expect(page).toHaveURL(/^[^?]*$/);
  await expect(hiding).toBeVisible();
});
