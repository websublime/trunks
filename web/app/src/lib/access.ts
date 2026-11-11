// How grants read to a person: who a grant is for, what it gives, and through which grant
// someone can use a server or a tool group.

import type { GrantSpec } from '../api/types.gen';
import { serverOf } from './group';

/** Names of users, teams and MCP clients: something, without spaces, 100 characters at most. */
export function identityNameProblem(name: string): string | undefined {
  if (name === '') return 'Enter a name.';
  if (/\s/.test(name)) return 'A name cannot contain spaces.';
  if ([...name].length > 100) return 'A name can have 100 characters at most.';
  return undefined;
}

/** "you", or "team platform". */
const holder = (grant: GrantSpec, username: string) =>
  grant.subject_kind === 'user' && grant.subject === username ? 'you' : `${grant.subject_kind} ${grant.subject}`;

const unique = (items: string[]) => [...new Set(items)];

/** Through whom this user can use the whole server: `["you", "team platform"]`. */
export function viaForServer(server: string, grants: GrantSpec[], username: string): string[] {
  return unique(
    grants
      .filter((g) => g.target_kind === 'server' && (g.target === server || g.target === '*'))
      .map((g) => holder(g, username)),
  );
}

/** Through whom this user can use the tool group. */
export function viaForGroup(group: string, grants: GrantSpec[], username: string): string[] {
  return unique(
    grants.filter((g) => g.target_kind === 'tool_group' && g.target === group).map((g) => holder(g, username)),
  );
}

/**
 * The tool groups through which some tools of a server are reached, for a server that was
 * not granted whole.
 */
export function groupsReaching(server: string, groups: { name: string; effective_tools: string[] }[]): string[] {
  return groups.filter((g) => g.effective_tools.some((tool) => serverOf(tool) === server)).map((g) => g.name);
}

/** What a grant gives, in words: "all servers", "server jira", "tool group triage". */
export function targetLabel(grant: Pick<GrantSpec, 'target_kind' | 'target'>): string {
  if (grant.target_kind === 'server') return grant.target === '*' ? 'all servers' : `server ${grant.target}`;
  return `tool group ${grant.target}`;
}

/** Orders grants for reading: by who holds them, then by what they give. */
export function byHolder(grants: GrantSpec[]): GrantSpec[] {
  const key = (g: GrantSpec) => `${g.subject_kind === 'team' ? 0 : 1} ${g.subject} ${g.target_kind} ${g.target}`;
  return [...grants].sort((a, b) => key(a).localeCompare(key(b)));
}
