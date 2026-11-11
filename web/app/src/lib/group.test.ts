import { describe, expect, it } from 'vitest';

import {
  effective,
  emptySelection,
  fromGroup,
  groupNameProblem,
  has,
  missing,
  serverOf,
  setServer,
  setTool,
  toGroup,
  type KnownTool,
} from './group';

const TOOLS: KnownTool[] = [
  { name: 'files__read', server: 'files', enabled: true },
  { name: 'files__write', server: 'files', enabled: true },
  { name: 'files__old', server: 'files', enabled: false },
  { name: 'notes__add', server: 'notes', enabled: true },
  { name: 'notes__list', server: 'notes', enabled: true },
];

describe('selection', () => {
  it('a whole server exposes its tools, minus the ones unticked', () => {
    let s = setServer(emptySelection(), 'files', true);
    expect(has(s, 'files__read')).toBe(true);
    expect(has(s, 'notes__add')).toBe(false);
    s = setTool(s, 'files__write', false);
    expect(toGroup('g', '', s)).toEqual({
      name: 'g',
      description: '',
      included_servers: ['files'],
      included_tools: [],
      excluded_tools: ['files__write'],
    });
    // the disabled tool is selected by the definition but not exposed
    expect(effective(s, TOOLS)).toEqual(['files__read']);
    // ticking it again removes the exclusion rather than adding an inclusion
    s = setTool(s, 'files__write', true);
    expect(toGroup('g', '', s).excluded_tools).toEqual([]);
    expect(toGroup('g', '', s).included_tools).toEqual([]);
  });

  it('outside an included server, tools are picked one by one', () => {
    let s = setTool(emptySelection(), 'notes__list', true);
    s = setTool(s, 'notes__add', true);
    s = setTool(s, 'notes__add', false);
    expect(toGroup(' g ', ' d ', s)).toEqual({
      name: 'g',
      description: 'd',
      included_servers: [],
      included_tools: ['notes__list'],
      excluded_tools: [],
    });
    expect(effective(s, TOOLS)).toEqual(['notes__list']);
  });

  it('including or dropping a server forgets the picks made inside it', () => {
    let s = setTool(setTool(emptySelection(), 'files__read', true), 'notes__add', true);
    s = setServer(s, 'files', true);
    expect([...s.included]).toEqual(['notes__add']);
    s = setTool(s, 'files__read', false);
    s = setServer(s, 'files', false);
    expect([...s.excluded]).toEqual([]);
    expect(has(s, 'files__read')).toBe(false);
    expect(has(s, 'notes__add')).toBe(true);
  });

  it('reads a stored definition and writes the same lists back, sorted', () => {
    const stored = { included_servers: ['notes', 'files'], included_tools: ['x__b', 'x__a'], excluded_tools: ['files__write'] };
    expect(toGroup('g', '', fromGroup(stored))).toEqual({
      name: 'g',
      description: '',
      included_servers: ['files', 'notes'],
      included_tools: ['x__a', 'x__b'],
      excluded_tools: ['files__write'],
    });
  });

  it('names what the gateway no longer has', () => {
    const s = fromGroup({ included_servers: ['files', 'gone'], included_tools: ['notes__add', 'gone__x'], excluded_tools: ['files__nope'] });
    expect(missing(s, ['files', 'notes'], TOOLS)).toEqual({
      servers: ['gone'],
      included: ['gone__x'],
      excluded: ['files__nope'],
    });
  });

  it('takes the server from a tool name', () => {
    expect(serverOf('files__read__more')).toBe('files');
    expect(serverOf('odd')).toBe('odd');
  });
});

describe('groupNameProblem', () => {
  it.each([
    ['triage', true],
    ['read-only_2', true],
    ['2fast', true],
    ['', false],
    ['-triage', false],
    ['_triage', false],
    ['tri age', false],
  ])('%j is accepted: %s', (name, accepted) => {
    expect(groupNameProblem(name) === undefined).toBe(accepted);
  });
});
