import { expect, test } from '@playwright/test';

import { accessible, ready, watch } from './console';
import { FAKE_UPSTREAM, Server } from './server';

let server: Server;

test.beforeAll(async () => {
  server = await Server.start('development');
});
test.afterAll(async () => {
  await server.stop();
});

test('the overview opens without sign-in and counts what the gateway holds', async ({ page }) => {
  const problems = watch(page);
  await page.goto(server.url);
  await ready(page);

  await expect(page).toHaveTitle('Overview · Trunks');
  await expect(page.getByRole('heading', { level: 1, name: 'Overview' })).toBeVisible();
  const header = page.getByRole('banner');
  await expect(header.getByText('Development', { exact: true })).toBeVisible();
  await expect(header.getByText('No sign-in. Everything is visible.')).toBeVisible();
  await expect(header.getByText(/^v\d+\.\d+\.\d+/)).toBeVisible();
  // Development mode has no users: nothing to sign out of.
  await expect(page.getByRole('button', { name: 'Sign out' })).toHaveCount(0);

  const card = (label: string) =>
    page.locator('.stat', { has: page.locator('.k', { hasText: new RegExp(`^${label}$`) }) }).locator('.v');
  await expect(card('Servers')).toHaveText('0 / 0');

  // A server registered with the CLI shows up on the next refresh.
  await server.cli('register', '--name', 'files', '--command', FAKE_UPSTREAM);
  await page.getByRole('button', { name: 'Refresh now' }).click();
  await expect(card('Servers')).toHaveText('1 / 1');
  await expect(card('Tools')).toHaveText('3 / 3');
  await expect(card('Prompts')).toHaveText('1');
  await expect(card('Resources')).toHaveText('1');
  await expect(card('Tool groups')).toHaveText('0');
  // In development everyone manages the gateway, so the installation's own facts show.
  await expect(card('Pending authorizations')).toHaveText('0');
  await expect(card('Open upstream sessions')).toBeVisible();

  await accessible(page);
  expect(problems).toEqual([]);
});

test('the light theme passes the same checks', async ({ browser }) => {
  const context = await browser.newContext({ colorScheme: 'light' });
  const page = await context.newPage();
  const problems = watch(page);
  await page.goto(server.url);
  await ready(page);
  await expect(page.getByRole('heading', { level: 1, name: 'Overview' })).toBeVisible();
  await accessible(page);
  expect(problems).toEqual([]);
  await context.close();
});

test('at phone width the navigation is a menu', async ({ browser }) => {
  const context = await browser.newContext({ viewport: { width: 390, height: 800 } });
  const page = await context.newPage();
  await page.goto(server.url);
  await ready(page);
  const side = page.getByRole('navigation', { name: 'Main', exact: true });
  await expect(side).toBeHidden();
  await page.locator('.mbar > summary').click();
  await expect(
    page.getByRole('navigation', { name: 'Main (menu)' }).getByRole('link', { name: 'Overview' }),
  ).toBeVisible();
  await accessible(page);
  await context.close();
});

test('pages are static files with a strict policy, and an unknown page says so', async ({ page, request }) => {
  const home = await request.get(server.url);
  expect(home.status()).toBe(200);
  const csp = home.headers()['content-security-policy'] ?? '';
  expect(csp).toContain("script-src 'self'");
  expect(csp).toContain("style-src 'self'");
  expect(csp).toContain("frame-ancestors 'none'");
  expect(csp).not.toContain('unsafe');
  expect(home.headers()['cache-control']).toBe('no-cache');

  const script = (await home.text()).match(/src="(\/_astro\/[^"]+\.js)"/)?.[1];
  expect(script).toBeTruthy();
  const asset = await request.get(server.url + script);
  expect(asset.headers()['cache-control']).toBe('public, max-age=31536000, immutable');

  const problems = watch(page);
  const missing = await page.goto(`${server.url}/no/such/page`);
  expect(missing?.status()).toBe(404);
  await ready(page);
  await expect(page.getByRole('heading', { name: 'Page not found' })).toBeVisible();
  await page.getByRole('link', { name: 'Go to the overview' }).click();
  await expect(page.getByRole('heading', { level: 1, name: 'Overview' })).toBeVisible();
  // the 404 itself is expected; nothing else may go wrong
  expect(problems.filter((p) => !p.includes('404'))).toEqual([]);

  // A mistyped API path is never answered with a page.
  const api = await request.get(`${server.url}/api/nope`);
  expect(api.status()).toBe(404);
  expect(await api.text()).toBe('');
});
