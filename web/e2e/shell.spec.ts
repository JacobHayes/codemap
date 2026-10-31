import { expect, test } from '@playwright/test';

test('the page loads the shell and the server answers the overview', async ({ page }) => {
  await page.goto('/');
  await expect(page.getByRole('banner')).toBeVisible();
  await expect(page.getByTestId('overview')).toHaveAttribute('data-state', 'ready');
});

test('an unknown route falls back to the app', async ({ page }) => {
  await page.goto('/some/deep/route');
  await expect(page.getByRole('banner')).toBeVisible();
});
