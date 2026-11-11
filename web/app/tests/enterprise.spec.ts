import { expect, test, type Page } from '@playwright/test';

import { accessible, ready, watch } from './console';
import { FAKE_UPSTREAM, Server } from './server';

const RUI = 'rui-token-0123456789';
const MARTA = 'marta-token-0123456789';

let server: Server;
let admin = '';

test.describe.configure({ mode: 'serial' });

test.beforeAll(async () => {
  server = await Server.start('enterprise');
});
test.afterAll(async () => {
  await server.stop();
});

async function signIn(page: Page, token: string): Promise<void> {
  await page.getByLabel('Access token').fill(token);
  await page.getByRole('button', { name: 'Sign in', exact: true }).click();
}

const card = (page: Page, label: string) =>
  page.locator('.stat', { has: page.locator('.k', { hasText: new RegExp(`^${label}$`) }) }).locator('.v');

test('before initialization every page says so, and opens the console once it is done', async ({ page }) => {
  const problems = watch(page);
  await page.goto(server.url);
  await expect(page).toHaveURL(`${server.url}/not-initialized/`);
  await expect(page.getByRole('heading', { name: 'The server is not initialized' })).toBeVisible();
  await accessible(page);

  // The sign-in page has nothing to offer yet either.
  await page.goto(`${server.url}/login/`);
  await expect(page).toHaveURL(`${server.url}/not-initialized/`);

  const out = await server.cli('init-server');
  admin = out.match(/Admin access token: (\S+)/)?.[1] ?? '';
  expect(admin).not.toBe('');

  await page.getByRole('button', { name: 'Check now' }).click();
  await expect(page).toHaveURL(`${server.url}/login/`);
  // the API answers 403 "not initialized" and then 401 on the way: both are expected here
  expect(problems.filter((p) => !/40[13]/.test(p))).toEqual([]);
});

test('sign-in: a wrong token is refused in place, the right one opens the console', async ({ page }) => {
  await server.cli('register', '--name', 'files', '--command', FAKE_UPSTREAM);
  await server.cli('register', '--name', 'other', '--command', FAKE_UPSTREAM);

  const problems = watch(page);
  await page.goto(server.url);
  await expect(page).toHaveURL(`${server.url}/login/`);
  await expect(page.getByRole('heading', { name: 'Sign in' })).toBeVisible();
  const field = page.getByLabel('Access token');
  await expect(field).toBeFocused();
  await expect(field).toHaveAttribute('type', 'password');
  // no single sign-on configured: no SSO button
  await expect(page.getByRole('link', { name: 'Sign in with SSO' })).toHaveCount(0);
  await accessible(page);

  await signIn(page, 'not-a-real-token');
  await expect(page.getByRole('alert')).toHaveText('That token is not valid. Check it and try again.');
  await expect(field).toHaveValue('');
  await expect(field).toBeFocused();
  await expect(field).toHaveAttribute('aria-invalid', 'true');
  await expect(page).toHaveURL(`${server.url}/login/`);
  await accessible(page);

  // "Show token" reveals what is being typed
  await field.fill('abc');
  await page.getByRole('button', { name: 'Show token' }).click();
  await expect(field).toHaveAttribute('type', 'text');

  await signIn(page, admin);
  await expect(page).toHaveURL(`${server.url}/`);
  await ready(page);
  const header = page.getByRole('banner');
  await expect(header.getByText('Enterprise', { exact: true })).toBeVisible();
  await expect(header.getByText('No sign-in. Everything is visible.')).toBeHidden();
  await expect(card(page, 'Servers')).toHaveText('2 / 2');
  await expect(card(page, 'Pending authorizations')).toHaveText('0');
  await expect(card(page, 'Open upstream sessions')).toBeVisible();
  await accessible(page);

  // The token is exchanged for a cookie and kept nowhere in the page's reach.
  const stored = await page.evaluate(
    () => JSON.stringify({ ...localStorage }) + JSON.stringify({ ...sessionStorage }) + document.cookie,
  );
  expect(stored).not.toContain(admin);
  const cookies = await page.context().cookies();
  expect(cookies.map((c) => [c.name, c.httpOnly])).toEqual([['trunks_session', true]]);

  // Sign out ends the session: the console is closed again.
  await header.locator('summary').click();
  await expect(header.getByText('Signed in as admin')).toBeVisible();
  await page.getByRole('button', { name: 'Sign out' }).click();
  await expect(page).toHaveURL(`${server.url}/login/`);
  await page.goto(server.url);
  await expect(page).toHaveURL(`${server.url}/login/`);
  // the refused sign-in and the signed-out checks answer 401: expected
  expect(problems.filter((p) => !p.includes('401'))).toEqual([]);
});

test('a user sees and counts only what they were granted', async ({ page }) => {
  await server.cli('create', 'user', 'rui', '--access-token', RUI);
  await server.cli('grant', '--user', 'rui', '--server', 'files');

  await page.goto(server.url);
  await signIn(page, RUI);
  await ready(page);
  await expect(page.getByRole('banner').locator('summary')).toContainText('rui');
  await expect(card(page, 'Servers')).toHaveText('1 / 1');
  await expect(card(page, 'Tools')).toHaveText('3 / 3');
  // Facts about the whole installation are not sent to a user, so they are not shown.
  await expect(card(page, 'Pending authorizations')).toHaveCount(0);
  await expect(card(page, 'Open upstream sessions')).toHaveCount(0);
  await accessible(page);
});

test('a user with no grants gets an explanation instead of an empty console', async ({ page }) => {
  await server.cli('create', 'user', 'marta', '--access-token', MARTA);

  await page.goto(server.url);
  await signIn(page, MARTA);
  await ready(page);
  await expect(page.getByText('You don’t have access to anything yet')).toBeVisible();
  await expect(page.getByText('Ask an administrator of this Trunks instance')).toBeVisible();
  await expect(page.locator('.stat .k')).toHaveText(['Username', 'Role', 'Teams']);
  await expect(page.locator('.stat', { hasText: 'Username' })).toContainText('marta');
  await expect(page.locator('.stat', { hasText: 'Teams' })).toContainText('None');
  await expect(card(page, 'Servers')).toHaveCount(0);
  await accessible(page);
});

test('sign-in returns to the page that was asked for, and never leaves the console', async ({ page }) => {
  await page.goto(`${server.url}/login/?return_to=${encodeURIComponent('https://example.com/')}`);
  await signIn(page, RUI);
  await expect(page).toHaveURL(`${server.url}/`);
});
