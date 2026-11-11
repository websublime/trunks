import { describe, expect, it } from 'vitest';

import type { GrantSpec } from '../api/types.gen';
import { byHolder, groupsReaching, identityNameProblem, targetLabel, viaForGroup, viaForServer } from './access';

const grant = (subject_kind: string, subject: string, target_kind: string, target: string): GrantSpec => ({
  subject_kind,
  subject,
  target_kind,
  target,
});

const GRANTS = [
  grant('user', 'rui', 'tool_group', 'triage'),
  grant('team', 'platform', 'server', '*'),
  grant('team', 'support', 'server', 'jira'),
  grant('team', 'support', 'tool_group', 'reporting'),
  grant('user', 'rui', 'server', 'jira'),
];

describe('via', () => {
  it('names every grant a server is reached through, the user first-person', () => {
    expect(viaForServer('jira', GRANTS, 'rui')).toEqual(['team platform', 'team support', 'you']);
    expect(viaForServer('github', GRANTS, 'rui')).toEqual(['team platform']);
    expect(viaForServer('github', GRANTS.slice(2), 'rui')).toEqual([]);
    // a grant to another user of the same name as a team is not "you"
    expect(viaForServer('jira', [grant('team', 'rui', 'server', 'jira')], 'rui')).toEqual(['team rui']);
  });

  it('does the same for tool groups, and never mixes the two kinds', () => {
    expect(viaForGroup('triage', GRANTS, 'rui')).toEqual(['you']);
    expect(viaForGroup('reporting', GRANTS, 'rui')).toEqual(['team support']);
    expect(viaForGroup('jira', GRANTS, 'rui')).toEqual([]);
    expect(viaForServer('triage', GRANTS.slice(0, 1), 'rui')).toEqual([]);
  });

  it('finds the groups that reach into a server', () => {
    const groups = [
      { name: 'triage', effective_tools: ['jira__search', 'github__list'] },
      { name: 'reporting', effective_tools: ['pg__query'] },
    ];
    expect(groupsReaching('jira', groups)).toEqual(['triage']);
    expect(groupsReaching('files', groups)).toEqual([]);
  });
});

describe('labels and order', () => {
  it.each([
    [grant('user', 'a', 'server', '*'), 'all servers'],
    [grant('user', 'a', 'server', 'jira'), 'server jira'],
    [grant('user', 'a', 'tool_group', 'triage'), 'tool group triage'],
  ])('%j → %s', (g, expected) => {
    expect(targetLabel(g)).toBe(expected);
  });

  it('lists teams before users, then by name and target', () => {
    expect(byHolder(GRANTS).map((g) => `${g.subject} ${g.target}`)).toEqual([
      'platform *',
      'support jira',
      'support reporting',
      'rui jira',
      'rui triage',
    ]);
  });
});

describe('identityNameProblem', () => {
  it.each([
    ['ana', true],
    ['ana.silva@example.com', true],
    ['', false],
    ['ana silva', false],
    ['a'.repeat(100), true],
    ['a'.repeat(101), false],
  ])('%j is accepted: %s', (name, accepted) => {
    expect(identityNameProblem(name) === undefined).toBe(accepted);
  });
});
