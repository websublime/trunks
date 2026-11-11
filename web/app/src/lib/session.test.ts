import { afterEach, describe, expect, it, vi } from 'vitest';

import type { WhoAmI } from '../api/types.gen';
import { hasNoAccess, loadSession, safeReturnTo, signInUrl, type Session } from './session';

/** A server answering each path with a status and a JSON body. */
function server(routes: Record<string, [number, unknown]>) {
  vi.stubGlobal(
    'fetch',
    vi.fn(async (input: RequestInfo | URL) => {
      const [status, body] = routes[String(input)] ?? [404, { error: 'not found' }];
      return new Response(JSON.stringify(body), { status });
    }),
  );
}

afterEach(() => vi.unstubAllGlobals());

const ana: WhoAmI = {
  username: 'ana',
  role: 'admin',
  teams: [],
  access: { all: true, servers: [], tool_groups: [] },
  grants: [],
};
const rui: WhoAmI = {
  username: 'rui',
  role: 'user',
  teams: [{ name: 'support', role: 'member' }],
  access: { all: false, servers: ['jira'], tool_groups: [] },
  grants: [{ subject_kind: 'team', subject: 'support', target_kind: 'server', target: 'jira' }],
};

describe('loadSession', () => {
  it('development: nobody signs in and everyone manages the gateway', async () => {
    server({
      '/auth/config': [200, { mode: 'development', login_required: false, oidc: false }],
      '/metadata': [200, { version: 'v1.0.0' }],
    });
    await expect(loadSession()).resolves.toEqual({
      kind: 'ready',
      session: { mode: 'development', oidc: false, admin: true, version: 'v1.0.0' },
    });
  });

  it('enterprise: the role comes from the server', async () => {
    for (const [user, admin] of [
      [ana, true],
      [rui, false],
    ] as const) {
      server({
        '/auth/config': [200, { mode: 'enterprise', login_required: true, oidc: true }],
        '/metadata': [200, { version: 'v1.0.0' }],
        '/api/v0/users/whoami': [200, user],
      });
      await expect(loadSession()).resolves.toEqual({
        kind: 'ready',
        session: { mode: 'enterprise', oidc: true, admin, user, version: 'v1.0.0' },
      });
    }
  });

  it('enterprise without a session: signed out, and whether SSO is offered', async () => {
    server({
      '/auth/config': [200, { mode: 'enterprise', login_required: true, oidc: true }],
      '/metadata': [200, { version: 'v1.0.0' }],
      '/api/v0/users/whoami': [401, { error: 'missing access token' }],
    });
    await expect(loadSession()).resolves.toEqual({ kind: 'signed-out', oidc: true, version: 'v1.0.0' });
  });

  it('a server nobody has initialized', async () => {
    server({
      '/auth/config': [200, { mode: null, login_required: false, oidc: false }],
      '/metadata': [200, { version: 'v1.0.0' }],
    });
    await expect(loadSession()).resolves.toEqual({ kind: 'uninitialized', version: 'v1.0.0' });
  });

  it('a failure that is not "signed out" is an error, never a session', async () => {
    server({
      '/auth/config': [200, { mode: 'enterprise', login_required: true, oidc: false }],
      '/metadata': [200, { version: 'v1.0.0' }],
      '/api/v0/users/whoami': [500, { error: 'database is unreachable' }],
    });
    await expect(loadSession()).rejects.toMatchObject({ status: 500 });
    server({ '/auth/config': [502, 'bad gateway'] });
    await expect(loadSession()).rejects.toMatchObject({ status: 502 });
  });

  it('the version is optional', async () => {
    server({ '/auth/config': [200, { mode: 'development', login_required: false, oidc: false }] });
    await expect(loadSession()).resolves.toMatchObject({ kind: 'ready', session: { version: '' } });
  });
});

describe('hasNoAccess', () => {
  const session = (user: WhoAmI | undefined, admin: boolean): Session => ({
    mode: 'enterprise',
    oidc: false,
    admin,
    version: '',
    ...(user ? { user } : {}),
  });
  const none = { all: false, servers: [], tool_groups: [] };

  it.each([
    ['a user with no grants', session({ ...rui, access: none }, false), true],
    ['a user with a server', session(rui, false), false],
    ['a user with a tool group', session({ ...rui, access: { ...none, tool_groups: ['triage'] } }, false), false],
    ['a user granted every server', session({ ...rui, access: { ...none, all: true } }, false), false],
    ['an administrator', session({ ...ana, access: none }, true), false],
    ['development mode (no user)', session(undefined, true), false],
  ])('%s', (_name, s, expected) => {
    expect(hasNoAccess(s)).toBe(expected);
  });
});

describe('safeReturnTo', () => {
  it.each([
    [null, '/'],
    ['', '/'],
    ['/', '/'],
    ['/servers/', '/servers/'],
    ['/servers/detail/?name=github', '/servers/detail/?name=github'],
    // never another origin
    ['https://evil.example/', '/'],
    ['//evil.example/', '/'],
    ['/\\evil.example/', '/'],
    ['javascript:alert(1)', '/'],
    ['servers/', '/'],
    // never a server endpoint
    ['/auth/logout', '/'],
    ['/api/v0/users', '/'],
    ['/mcp', '/'],
    ['/init', '/'],
    ['/metrics', '/'],
  ])('%s → %s', (value, expected) => {
    expect(safeReturnTo(value)).toBe(expected);
  });
});

describe('signInUrl', () => {
  it.each([
    ['/', '/login/'],
    ['/servers/', '/login/?return_to=%2Fservers%2F'],
    ['/servers/detail/?name=a b', '/login/?return_to=%2Fservers%2Fdetail%2F%3Fname%3Da%20b'],
  ])('%s', (from, expected) => {
    expect(signInUrl(from)).toBe(expected);
  });
});
