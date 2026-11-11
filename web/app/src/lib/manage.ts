// The changes an administrator makes from the console (anyone, in development mode). Each
// is one call of the REST API; the views load their data again afterwards.

import { del, post, put } from '../api/client';
import type {
  CreateMcpClientRequest,
  GrantSpec,
  McpClientWithToken,
  RegisterServerInput,
  RegisterServerResult,
  Team,
  ToolGroup,
  UserWithToken,
} from '../api/types.gen';

export type EntityKind = 'tools' | 'prompts' | 'resources';

const name = encodeURIComponent;
const verb = (on: boolean) => (on ? 'enable' : 'disable');

export const setServer = (server: string, on: boolean) => post(`/api/v0/servers/${name(server)}/${verb(on)}`);

/** `entity` is a tool or prompt name, or a resource's `trunks://` URI. */
export const setEntity = (kind: EntityKind, entity: string, on: boolean) =>
  post(`/api/v0/${kind}/${verb(on)}?entity=${name(entity)}`);

export const registerServer = (definition: RegisterServerInput) =>
  post<RegisterServerResult>('/api/v0/servers', definition);

export const deleteServer = (server: string) => del(`/api/v0/servers/${name(server)}`);

export const createGroup = (group: ToolGroup) => post('/api/v0/tool-groups', group);
export const updateGroup = (group: ToolGroup) => put(`/api/v0/tool-groups/${name(group.name)}`, group);
export const deleteGroup = (group: string) => del(`/api/v0/tool-groups/${name(group)}`);

// ---- identities and access (enterprise mode)

export const createUser = (username: string) => post<UserWithToken>('/api/v0/users', { username });
/** Replaces the user's token; the old one stops working at once. */
export const rotateUserToken = (username: string) => put<UserWithToken>(`/api/v0/users/${name(username)}`, {});
export const deleteUser = (username: string) => del(`/api/v0/users/${name(username)}`);

export const createClient = (client: CreateMcpClientRequest) => post<McpClientWithToken>('/api/v0/clients', client);
export const rotateClientToken = (client: string) =>
  put<McpClientWithToken>(`/api/v0/clients/${name(client)}`, { rotate_token: true });
export const deleteClient = (client: string) => del(`/api/v0/clients/${name(client)}`);

export const createTeam = (team: Team) => post('/api/v0/teams', team);
export const deleteTeam = (team: string) => del(`/api/v0/teams/${name(team)}`);
/** Adds a member, or changes the role of one. */
export const setMember = (team: string, username: string, role: string) =>
  post(`/api/v0/teams/${name(team)}/members`, { username, role });
export const removeMember = (team: string, username: string) =>
  del(`/api/v0/teams/${name(team)}/members/${name(username)}`);

export const addGrant = (grant: GrantSpec) => post('/api/v0/grants', grant);
export const revokeGrant = (grant: GrantSpec) => del('/api/v0/grants', grant);
