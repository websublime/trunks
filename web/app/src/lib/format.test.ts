import { describe, expect, it } from 'vitest';

import { bareVersion, clock, modeLabel, uptime } from './format';
import { page, visibleTo } from './nav';

describe('uptime', () => {
  it.each([
    [0, '0 s'],
    [45, '45 s'],
    [60, '1 min'],
    [3_599, '59 min'],
    [3_600, '1 h 0 min'],
    [15_120, '4 h 12 min'],
    [86_400, '1 d 0 h 0 min'],
    [274_320, '3 d 4 h 12 min'],
    [-5, '0 s'],
    [59.9, '59 s'],
  ])('%d s → %s', (seconds, expected) => {
    expect(uptime(seconds)).toBe(expected);
  });
});

describe('text helpers', () => {
  it('formats a clock time with two digits each', () => {
    expect(clock(new Date(2026, 0, 2, 3, 4, 5))).toBe('03:04:05');
    expect(clock(new Date(2026, 0, 2, 12, 4, 31))).toBe('12:04:31');
  });

  it('shows the version as a value', () => {
    expect(bareVersion('v1.2.3')).toBe('1.2.3');
    expect(bareVersion('v1.2.3+abc1234')).toBe('1.2.3+abc1234');
    expect(bareVersion('1.2.3')).toBe('1.2.3');
  });

  it('names the modes', () => {
    expect(modeLabel('development')).toBe('Development');
    expect(modeLabel('enterprise')).toBe('Enterprise');
  });
});

describe('navigation', () => {
  it('links only to pages the console has', () => {
    expect(page('/')).toBe('/');
    expect(page('/no-such-page/')).toBeUndefined();
  });

  it.each([
    ['everyone', { enterprise: false, admin: true }, true],
    ['everyone', { enterprise: true, admin: false }, true],
    ['enterprise', { enterprise: false, admin: true }, false],
    ['enterprise', { enterprise: true, admin: false }, true],
    ['admin', { enterprise: true, admin: false }, false],
    ['admin', { enterprise: false, admin: true }, true],
    ['enterprise-admin', { enterprise: false, admin: true }, false],
    ['enterprise-admin', { enterprise: true, admin: false }, false],
    ['enterprise-admin', { enterprise: true, admin: true }, true],
  ] as const)('%s for %o → %s', (audience, viewer, expected) => {
    expect(visibleTo(audience, viewer)).toBe(expected);
  });
});
