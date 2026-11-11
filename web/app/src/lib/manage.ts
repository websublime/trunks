// The changes an administrator makes from the console (anyone, in development mode). Each
// is one call of the REST API; the views load their data again afterwards.

import { del, post, put } from '../api/client';
import type { RegisterServerInput, RegisterServerResult, ToolGroup } from '../api/types.gen';

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
