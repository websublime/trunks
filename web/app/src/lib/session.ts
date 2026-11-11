// Who is looking at the console, worked out from the server on every page load. The pages
// are static shells, the same for everyone; this is what turns one into "ana's console".

import { ApiError, get } from '../api/client';
import type { AuthConfig, ServerMetadata, ServerMode, WhoAmI } from '../api/types.gen';

export interface Session {
  mode: ServerMode;
  /** Single sign-on is configured. */
  oidc: boolean;
  /** May manage the gateway: an administrator, or anyone in development mode. */
  admin: boolean;
  /** The signed-in user; absent in development mode, which has none. */
  user?: WhoAmI;
  /** Server version, e.g. `v1.2.3`. Empty when it could not be read. */
  version: string;
}

export type SessionState =
  | { kind: 'ready'; session: Session }
  | { kind: 'uninitialized'; version: string }
  | { kind: 'signed-out'; oidc: boolean; version: string };

async function version(): Promise<string> {
  try {
    return (await get<ServerMetadata>('/metadata')).version;
  } catch {
    return '';
  }
}

/** Asks the server who is here. Throws `ApiError` only when the server cannot answer. */
export async function loadSession(): Promise<SessionState> {
  const [config, v] = await Promise.all([get<AuthConfig>('/auth/config'), version()]);
  if (config.mode === null) return { kind: 'uninitialized', version: v };
  if (!config.login_required) {
    return { kind: 'ready', session: { mode: config.mode, oidc: false, admin: true, version: v } };
  }
  try {
    const user = await get<WhoAmI>('/api/v0/users/whoami');
    return {
      kind: 'ready',
      session: {
        mode: config.mode,
        oidc: config.oidc,
        admin: user.role === 'admin',
        user,
        version: v,
      },
    };
  } catch (e) {
    if (e instanceof ApiError && e.signedOut) {
      return { kind: 'signed-out', oidc: config.oidc, version: v };
    }
    throw e;
  }
}

/** A user who has been granted nothing: the console has nothing to show them. */
export function hasNoAccess(session: Session): boolean {
  const access = session.user?.access;
  if (session.admin || access === undefined) return false;
  return !access.all && access.servers.length === 0 && access.tool_groups.length === 0;
}

/**
 * Where to go after signing in: only a path inside this console. Anything else (another
 * origin, a protocol-relative URL, a server path) falls back to the overview.
 */
export function safeReturnTo(value: string | null): string {
  if (value === null || !value.startsWith('/') || value.startsWith('//') || value.includes('\\')) {
    return '/';
  }
  const first = value.slice(1).split(/[/?#]/)[0];
  const server = ['api', 'auth', 'mcp', 'v0', 'health', 'metadata', 'metrics', 'init'];
  return server.includes(first ?? '') ? '/' : value;
}

export function signInUrl(returnTo: string): string {
  return returnTo === '/' ? '/login/' : `/login/?return_to=${encodeURIComponent(returnTo)}`;
}
