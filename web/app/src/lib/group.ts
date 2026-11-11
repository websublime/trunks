// A tool group as the form sees it: per server, either the whole server (minus exclusions)
// or tools picked one by one. Converts to and from the three lists the API stores.

import type { ToolGroup } from '../api/types.gen';

export interface KnownTool {
  name: string;
  server: string;
  enabled: boolean;
}

export interface Selection {
  /** Servers included whole: every enabled tool, also ones added later. */
  servers: ReadonlySet<string>;
  /** Tools included by name. */
  included: ReadonlySet<string>;
  /** Tools left out of an included server. */
  excluded: ReadonlySet<string>;
}

/** The server a canonical tool name belongs to. */
export const serverOf = (tool: string) => tool.split('__')[0] ?? '';

export const emptySelection = (): Selection => ({ servers: new Set(), included: new Set(), excluded: new Set() });

export function fromGroup(group: Pick<ToolGroup, 'included_servers' | 'included_tools' | 'excluded_tools'>): Selection {
  return {
    servers: new Set(group.included_servers),
    included: new Set(group.included_tools),
    excluded: new Set(group.excluded_tools),
  };
}

const without = (set: ReadonlySet<string>, drop: (item: string) => boolean) =>
  new Set([...set].filter((item) => !drop(item)));

/**
 * Includes or drops a whole server. Either way the picks made tool by tool for that server
 * stop meaning anything and are forgotten.
 */
export function setServer(s: Selection, server: string, on: boolean): Selection {
  const ofServer = (tool: string) => serverOf(tool) === server;
  const servers = new Set(s.servers);
  if (on) servers.add(server);
  else servers.delete(server);
  return { servers, included: without(s.included, ofServer), excluded: without(s.excluded, ofServer) };
}

/** Whether the group exposes this tool, as far as the definition goes. */
export function has(s: Selection, tool: string): boolean {
  if (s.excluded.has(tool)) return false;
  return s.servers.has(serverOf(tool)) || s.included.has(tool);
}

/** Ticks or unticks one tool: an exclusion inside an included server, a pick outside one. */
export function setTool(s: Selection, tool: string, on: boolean): Selection {
  const included = new Set(s.included);
  const excluded = new Set(s.excluded);
  if (s.servers.has(serverOf(tool))) {
    included.delete(tool);
    if (on) excluded.delete(tool);
    else excluded.add(tool);
  } else {
    excluded.delete(tool);
    if (on) included.add(tool);
    else included.delete(tool);
  }
  return { servers: s.servers, included, excluded };
}

/** The tools the group would expose right now: what is selected, exists and is enabled. */
export function effective(s: Selection, tools: KnownTool[]): string[] {
  return tools.filter((t) => t.enabled && has(s, t.name)).map((t) => t.name);
}

/** Names the definition refers to that the gateway does not have (any more). */
export function missing(s: Selection, servers: string[], tools: KnownTool[]) {
  const names = new Set(tools.map((t) => t.name));
  return {
    servers: [...s.servers].filter((name) => !servers.includes(name)).sort(),
    included: [...s.included].filter((name) => !names.has(name)).sort(),
    excluded: [...s.excluded].filter((name) => !names.has(name)).sort(),
  };
}

export function toGroup(name: string, description: string, s: Selection): ToolGroup {
  return {
    name: name.trim(),
    description: description.trim(),
    included_servers: [...s.servers].sort(),
    included_tools: [...s.included].sort(),
    excluded_tools: [...s.excluded].sort(),
  };
}

/** The server's rule for a group name, with the reason when it is not met. */
export function groupNameProblem(name: string): string | undefined {
  if (name === '') return 'Enter a name.';
  if (!/^[a-zA-Z0-9][a-zA-Z0-9_-]*$/.test(name)) {
    return 'Start with a letter or a digit; then letters, digits, hyphens and underscores.';
  }
  return undefined;
}
