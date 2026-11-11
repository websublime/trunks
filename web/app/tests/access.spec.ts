// Identities and access in enterprise mode: users, teams, grants, MCP clients and the
// account page. Tokens are checked for real: the one shown is the one that works.

import { expect, test, type APIRequestContext, type Page } from '@playwright/test';

import { accessible, ready, watch } from './console';
import { FAKE_UPSTREAM, Server } from './server';

const RUI = 'rui-token-0123456789';
const rows = (page: Page) => page.locator('table.tbl > tbody > tr');
const main = (page: Page) => page.getByRole('main');
const notice = (page: Page, text: string) => page.getByRole('status').filter({ hasText: text });

/** What the MCP endpoint answers to an `initialize` with this token. */
async function mcp(request: APIRequestContext, url: string, token?: string): Promise<number> {
  const response = await request.post(`${url}/mcp`, {
    headers: {
      accept: 'application/json, text/event-stream',
      ...(token === undefined ? {} : { authorization: `Bearer ${token}` }),
    },
    data: {
      jsonrpc: '2.0',
      id: 1,
      method: 'initialize',
      params: { protocolVersion: '2025-06-18', capabilities: {}, clientInfo: { name: 'smoke', version: '0' } },
    },
  });
  return response.status();
}

/** Whether a user token is accepted for signing in. */
async function signsIn(request: APIRequestContext, url: string, token: string): Promise<number> {
  return (await request.post(`${url}/auth/token`, { data: { access_token: token } })).status();
}

/** Reads the token out of the "shown once" dialog and closes it. */
async function takeToken(page: Page, owner: string): Promise<string> {
  const dialog = page.getByRole('dialog', { name: `Access token for ${owner}` });
  await expect(dialog).toBeVisible();
  const token = (await dialog.locator('[data-token]').innerText()).trim();
  expect(token.length).toBeGreaterThanOrEqual(16);
  await dialog.getByRole('button', { name: 'Done' }).click();
  await expect(dialog).toBeHidden();
  // gone from the page with the dialog
  await expect(page.locator('body')).not.toContainText(token);
  return token;
}

test.describe('enterprise', () => {
  test.describe.configure({ mode: 'serial' });

  let server: Server;
  let admin = '';

  test.beforeAll(async () => {
    server = await Server.start('enterprise');
    admin = (await server.cli('init-server')).match(/Admin access token: (\S+)/)?.[1] ?? '';
    await server.cli('register', '--name', 'files', '--command', FAKE_UPSTREAM);
    await server.cli('register', '--name', 'jira', '--command', FAKE_UPSTREAM);
    await server.cli(
      'create',
      'group',
      '-c',
      await server.file('triage.json', { name: 'triage', included_servers: ['jira'] }),
    );
    await server.cli('create', 'user', 'rui', '--access-token', RUI);
    await server.cli('create', 'team', 'platform', '--description', 'Runs the gateway');
    await server.cli('add-team-member', 'platform', 'rui', '--role', 'maintainer');
    await server.cli('grant', '--team', 'platform', '--server', 'files');
    await server.cli('grant', '--user', 'rui', '--group', 'triage');
  });
  test.afterAll(async () => {
    await server.stop();
  });

  async function signIn(page: Page, token: string): Promise<void> {
    await page.goto(`${server.url}/login/`);
    await page.getByLabel('Access token').fill(token);
    await page.getByRole('button', { name: 'Sign in', exact: true }).click();
    await expect(page).toHaveURL(`${server.url}/`);
    await ready(page);
  }

  test('users: added with a token shown once, which can be replaced; deleted', async ({ page, context, request }) => {
    await context.grantPermissions(['clipboard-read', 'clipboard-write']);
    await signIn(page, admin);
    const problems = watch(page);
    await page.getByRole('navigation', { name: 'Main', exact: true }).getByRole('link', { name: 'Users' }).click();
    await expect(page).toHaveURL(`${server.url}/users/`);
    await ready(page);
    await expect(page).toHaveTitle('Users · Trunks');
    await expect(rows(page)).toHaveCount(2);
    await accessible(page);

    const add = page.getByRole('button', { name: 'Add user' });
    await add.click();
    await expect(main(page).getByText('Enter a name.')).toBeVisible();
    await page.getByLabel('Username').fill('marta silva');
    await add.click();
    await expect(main(page).getByText('A name cannot contain spaces.')).toBeVisible();
    await page.getByLabel('Username').fill('rui');
    await add.click();
    await expect(main(page).getByText('A user of this name already exists.')).toBeVisible();
    await accessible(page);

    await page.getByLabel('Username').fill('marta');
    await add.click();
    const dialog = page.getByRole('dialog', { name: 'Access token for marta' });
    await expect(dialog).toBeVisible();
    await expect(dialog.getByText('Shown once')).toBeVisible();
    await accessible(page);
    // no way out by accident
    await page.keyboard.press('Escape');
    await expect(dialog).toBeVisible();
    await dialog.getByRole('button', { name: 'Copy token' }).click();
    await expect(dialog.getByText('Copied.')).toBeVisible();
    const copied = await page.evaluate(() => navigator.clipboard.readText());
    const first = await takeToken(page, 'marta');
    expect(copied).toBe(first);
    await expect(notice(page, 'User marta added.')).toBeVisible();
    await expect(rows(page).filter({ hasText: 'marta' })).toContainText('user');
    expect(await signsIn(request, server.url, first)).toBe(200);
    // kept nowhere the page could read it back from
    const stored = await page.evaluate(() => JSON.stringify({ ...localStorage }) + JSON.stringify({ ...sessionStorage }));
    expect(stored).not.toContain(first);

    await page.getByRole('button', { name: 'New token for marta' }).click();
    const sure = page.getByRole('alertdialog', { name: 'Replace the token of marta?' });
    await expect(sure).toContainText('stops working at once');
    await sure.getByRole('button', { name: 'Replace token' }).click();
    const second = await takeToken(page, 'marta');
    expect(second).not.toBe(first);
    expect(await signsIn(request, server.url, first)).toBe(401);
    expect(await signsIn(request, server.url, second)).toBe(200);

    // An administrator cannot delete themselves from here; anyone else, after confirming.
    const self = rows(page).filter({ hasText: 'admin' }).first();
    await expect(self.getByRole('button', { name: /^Delete/ })).toHaveCount(0);
    await page.getByRole('button', { name: 'Delete marta' }).click();
    await page.getByRole('alertdialog', { name: 'Delete marta?' }).getByRole('button', { name: 'Delete user' }).click();
    await expect(notice(page, 'User marta deleted.')).toBeVisible();
    await expect(rows(page)).toHaveCount(2);
    expect(await signsIn(request, server.url, second)).toBe(401);
    await accessible(page);
    // the name already taken was answered 400: expected
    expect(problems.filter((p) => !p.includes('400'))).toEqual([]);
  });

  test('teams: created, members added, a role changed, a member removed, deleted', async ({ page }) => {
    await signIn(page, admin);
    await page.goto(`${server.url}/teams/`);
    await ready(page);
    await expect(page).toHaveTitle('Teams · Trunks');
    await expect(rows(page)).toHaveCount(1);
    await expect(rows(page)).toContainText('rui· maintainer');
    await page.getByLabel('Name').fill('support');
    await page.getByLabel('Description').fill('Customer support');
    await page.getByRole('button', { name: 'Create team' }).click();
    await expect(notice(page, 'Team support created.')).toBeVisible();
    await expect(rows(page)).toHaveCount(2);
    await accessible(page);

    await rows(page).getByRole('link', { name: 'support' }).click();
    await expect(page).toHaveURL(`${server.url}/teams/detail/?name=support`);
    await ready(page);
    await expect(main(page).getByText('No members yet')).toBeVisible();
    await accessible(page);

    // someone who does not exist: the server's answer, in place
    await page.getByLabel('User', { exact: true }).fill('ghost');
    await page.getByRole('button', { name: 'Add member' }).click();
    await expect(page.getByRole('alert').filter({ hasText: 'Couldn’t add ghost' })).toBeVisible();

    await page.getByLabel('User', { exact: true }).fill('rui');
    await page.getByLabel('Role', { exact: true }).selectOption('maintainer');
    await page.getByRole('button', { name: 'Add member' }).click();
    await expect(notice(page, 'rui added to support as maintainer.')).toBeVisible();
    const role = page.getByLabel('Role of rui');
    await expect(role).toHaveValue('maintainer');
    await role.selectOption('member');
    await expect
      .poll(async () => JSON.stringify(await (await page.request.get(`${server.url}/api/v0/teams/support`)).json()))
      .toContain('"role":"member"');
    await accessible(page);

    await page.getByRole('button', { name: 'Remove rui' }).click();
    await page.getByRole('alertdialog', { name: 'Remove rui from support?' }).getByRole('button', { name: 'Remove' }).click();
    await expect(notice(page, 'rui removed from support.')).toBeVisible();
    await expect(main(page).getByText('No members yet')).toBeVisible();

    await page.getByRole('button', { name: 'Delete' }).click();
    const sure = page.getByRole('alertdialog', { name: 'Delete support?' });
    await expect(sure).toContainText('MCP clients it owns');
    await sure.getByRole('button', { name: 'Delete team' }).click();
    await expect(page).toHaveURL(`${server.url}/teams/`);
    await ready(page);
    await expect(notice(page, 'Team support deleted.')).toBeVisible();
    await expect(rows(page)).toHaveCount(1);
  });

  test('access: grants are added and revoked', async ({ page }) => {
    await signIn(page, admin);
    const problems = watch(page);
    await page.goto(`${server.url}/access/`);
    await ready(page);
    await expect(page).toHaveTitle('Access · Trunks');
    await expect(rows(page)).toHaveCount(2);
    await accessible(page);

    const add = page.getByRole('button', { name: 'Add grant' });
    await add.click();
    await expect(page.getByRole('alert').filter({ hasText: 'Choose a user or a team.' })).toBeVisible();
    // administrators are not offered: a grant to one would change nothing
    await expect(page.getByLabel('User or team').locator('option')).toHaveText(['Choose…', 'platform', 'rui']);

    await page.getByLabel('User or team').selectOption('team:platform');
    await page.getByLabel('Server', { exact: true }).selectOption('files');
    await add.click();
    await expect(page.getByRole('alert').filter({ hasText: 'Team platform already has that grant.' })).toBeVisible();

    await page.getByLabel('Server', { exact: true }).selectOption('jira');
    await add.click();
    await expect(notice(page, 'Team platform can now use server jira.')).toBeVisible();
    await expect(rows(page)).toHaveCount(3);

    await page.getByRole('button', { name: 'All servers' }).click();
    await page.getByLabel('User or team').selectOption('user:rui');
    await add.click();
    await expect(notice(page, 'User rui can now use all servers.')).toBeVisible();
    await expect(rows(page).filter({ hasText: 'All servers' })).toHaveCount(1);
    expect(await server.cli('list', 'grants')).toContain('*');
    await accessible(page);

    await page.getByRole('button', { name: 'Revoke all servers from user rui' }).click();
    const sure = page.getByRole('alertdialog', { name: 'Revoke access?' });
    await expect(sure).toContainText('User rui will lose all servers');
    await accessible(page);
    await sure.getByRole('button', { name: 'Revoke' }).click();
    await expect(rows(page).filter({ hasText: 'All servers' })).toHaveCount(0);
    await page.getByRole('button', { name: 'Revoke server jira from team platform' }).click();
    await sure.getByRole('button', { name: 'Revoke' }).click();
    await expect(rows(page)).toHaveCount(2);
    expect(problems).toEqual([]);
  });

  test('MCP clients: the token shown is the one that opens the MCP endpoint', async ({ page, request }) => {
    await signIn(page, admin);
    const problems = watch(page);
    await page.goto(`${server.url}/clients/`);
    await ready(page);
    await expect(page).toHaveTitle('MCP clients · Trunks');
    await expect(main(page).getByText('No MCP clients yet')).toBeVisible();
    await main(page).getByRole('link', { name: 'New client' }).first().click();
    await expect(page).toHaveURL(`${server.url}/clients/new/`);
    await ready(page);
    await accessible(page);

    const create = page.getByRole('button', { name: 'Create client' });
    await create.click();
    await expect(main(page).getByText('Enter a name.')).toBeVisible();
    await page.getByLabel('Name (required)').fill('ci-agent');
    await page.getByLabel('Description').fill('Release notes agent');
    await page.getByLabel('A team').check();
    await expect(page.getByLabel('Team', { exact: true })).toHaveValue('platform');
    await page.getByLabel('Only these servers, as far as its owner can use them').check();
    await create.click();
    await expect(page.getByRole('alert').filter({ hasText: 'Tick at least one server.' })).toBeVisible();
    await page.getByLabel('files', { exact: true }).check();
    await accessible(page);
    await create.click();
    const first = await takeToken(page, 'ci-agent');
    await expect(page).toHaveURL(`${server.url}/clients/`);
    await ready(page);
    await expect(notice(page, 'MCP client ci-agent created.')).toBeVisible();
    const row = rows(page).filter({ hasText: 'ci-agent' });
    await expect(row).toContainText('platform');
    await expect(row).toContainText('files');
    expect(await mcp(request, server.url)).toBe(401);
    expect(await mcp(request, server.url, first)).toBe(200);
    // a user's token is not an MCP client's
    expect(await mcp(request, server.url, RUI)).toBe(401);

    // one with no owner reaches exactly what it is told to
    await page.goto(`${server.url}/clients/new/`);
    await ready(page);
    await page.getByLabel('Name (required)').fill('ci-agent');
    await page.getByLabel('Nobody').check();
    await expect(page.getByLabel('Every server')).toBeChecked();
    await create.click();
    await expect(main(page).getByText('An MCP client of this name already exists.')).toBeVisible();
    await page.getByLabel('Name (required)').fill('bot');
    await create.click();
    await takeToken(page, 'bot');
    await ready(page);
    await expect(rows(page).filter({ hasText: 'bot' })).toContainText('All servers');
    await accessible(page);

    await page.getByRole('button', { name: 'New token for ci-agent' }).click();
    await page
      .getByRole('alertdialog', { name: 'Replace the token of ci-agent?' })
      .getByRole('button', { name: 'Replace token' })
      .click();
    const second = await takeToken(page, 'ci-agent');
    expect(second).not.toBe(first);
    expect(await mcp(request, server.url, first)).toBe(401);
    expect(await mcp(request, server.url, second)).toBe(200);

    await page.getByRole('button', { name: 'Delete bot' }).click();
    await page.getByRole('alertdialog', { name: 'Delete bot?' }).getByRole('button', { name: 'Delete client' }).click();
    await expect(notice(page, 'MCP client bot deleted.')).toBeVisible();
    await expect(rows(page)).toHaveCount(1);
    // the name already taken was answered 400: expected
    expect(problems.filter((p) => !p.includes('400'))).toEqual([]);
  });

  test('a user: what they can use and why, their own clients, and nothing of the administration', async ({ page, request }) => {
    await signIn(page, RUI);
    const navigation = page.getByRole('navigation', { name: 'Main', exact: true });
    await expect(navigation.getByRole('link', { name: 'MCP clients' })).toBeVisible();
    await expect(navigation.getByRole('link', { name: 'Teams' })).toBeVisible();
    for (const hidden of ['Users', 'Access', 'Diagnostics']) {
      await expect(navigation.getByRole('link', { name: hidden, exact: true })).toBeHidden();
    }

    // the account page is reached from the user menu
    await page.getByRole('banner').locator('summary').click();
    await page.getByRole('banner').getByRole('link', { name: 'Account' }).click();
    await expect(page).toHaveURL(`${server.url}/account/`);
    await ready(page);
    await expect(page).toHaveTitle('Account · Trunks');
    const servers = page.locator('.panel', { hasText: 'Servers you can use' });
    await expect(servers.locator('li').filter({ hasText: 'files' })).toContainText('via team platform');
    await expect(servers.locator('li').filter({ hasText: 'jira' })).toContainText('some tools, via tool group triage');
    await expect(page.locator('.panel', { hasText: 'Tool groups you can use' }).locator('li')).toContainText('via you');
    const you = page.locator('.panel', { hasText: 'Username' });
    await expect(you).toContainText('rui');
    await expect(you).toContainText('platform· maintainer');
    await accessible(page);

    // their teams' clients, which a maintainer may manage, and their own
    await page.goto(`${server.url}/clients/`);
    await ready(page);
    await expect(rows(page)).toHaveCount(1);
    await expect(page.getByRole('button', { name: 'New token for ci-agent' })).toBeVisible();
    await main(page).getByRole('link', { name: 'New client' }).click();
    await ready(page);
    await expect(page.getByLabel('Me', { exact: true })).toBeChecked();
    await expect(page.getByLabel('A team I maintain')).toBeVisible();
    await expect(page.getByLabel('Nobody')).toHaveCount(0);
    await page.getByLabel('Name (required)').fill('desk-rui');
    await accessible(page);
    await page.getByRole('button', { name: 'Create client' }).click();
    const token = await takeToken(page, 'desk-rui');
    await ready(page);
    await expect(rows(page).filter({ hasText: 'desk-rui' })).toContainText('Everything its owner can use');
    expect(await mcp(request, server.url, token)).toBe(200);

    // a maintainer changes the membership of their team, without a list of everyone
    await page.goto(`${server.url}/teams/detail/?name=platform`);
    await ready(page);
    await expect(page.getByRole('button', { name: 'Add member' })).toBeVisible();
    await expect(page.getByRole('button', { name: 'Delete', exact: true })).toHaveCount(0);
    await expect(page.locator('#m-users option')).toHaveCount(0);
    await accessible(page);

    for (const [path, what] of [
      ['/users/', 'users'],
      ['/access/', 'grants'],
    ]) {
      await page.goto(server.url + path);
      await ready(page);
      await expect(main(page).getByText(`You can’t see ${what}`)).toBeVisible();
    }
    await accessible(page);

    await page.goto(`${server.url}/account/`);
    await ready(page);
    await main(page).getByRole('button', { name: 'Sign out' }).click();
    await expect(page).toHaveURL(`${server.url}/login/`);
  });

  test('the same pages at phone width, in the light theme', async ({ browser }) => {
    const context = await browser.newContext({ viewport: { width: 390, height: 800 }, colorScheme: 'light' });
    const page = await context.newPage();
    await signIn(page, admin);
    for (const path of ['/clients/', '/clients/new/', '/users/', '/teams/', '/teams/detail/?name=platform', '/access/', '/account/']) {
      await page.goto(server.url + path);
      await ready(page);
      await expect(page.locator('.ph h1')).toBeVisible();
      await expect(page.getByRole('main').locator('.panel, form').first()).toBeVisible();
      const overflow = await page.evaluate(
        () => document.documentElement.scrollWidth - document.documentElement.clientWidth,
      );
      expect(overflow, path).toBeLessThanOrEqual(0);
      await accessible(page);
    }
    await context.close();
  });
});

test('development mode has no identities: the pages say so and the navigation leaves them out', async ({ page }) => {
  const server = await Server.start('development');
  try {
    await page.goto(server.url);
    await ready(page);
    const navigation = page.getByRole('navigation', { name: 'Main', exact: true });
    for (const hidden of ['MCP clients', 'Users', 'Teams', 'Access']) {
      await expect(navigation.getByRole('link', { name: hidden, exact: true })).toBeHidden();
    }
    for (const path of ['/clients/', '/clients/new/', '/users/', '/teams/', '/access/', '/account/']) {
      await page.goto(server.url + path);
      await ready(page);
      await expect(page.getByRole('main').getByText('Only in enterprise mode')).toBeVisible();
    }
    await accessible(page);
  } finally {
    await server.stop();
  }
});
