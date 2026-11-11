import type { JsonValue } from '../api/types.gen';
import { describe, expect, it } from 'vitest';

import { promptArguments, toolParameters, typeOf } from './schema';
import { count, middle } from './text';

describe('toolParameters', () => {
  it('reads names, types, required and descriptions from a JSON Schema', () => {
    const schema = {
      type: 'object',
      properties: {
        owner: { type: 'string', description: 'Repository owner' },
        labels: { type: 'array', items: { type: 'string' } },
        limit: { type: ['integer', 'null'] },
        state: { enum: ['open', 'closed'] },
        filter: { anyOf: [{ type: 'string' }, { type: 'object' }] },
        anything: {},
      },
      required: ['owner', 'limit'],
    };
    expect(toolParameters(schema)).toEqual([
      { name: 'owner', type: 'string', required: true, description: 'Repository owner' },
      { name: 'labels', type: 'array of string', required: false, description: '' },
      { name: 'limit', type: 'integer or null', required: true, description: '' },
      { name: 'state', type: 'enum', required: false, description: '' },
      { name: 'filter', type: 'string or object', required: false, description: '' },
      { name: 'anything', type: 'any', required: false, description: '' },
    ]);
  });

  it.each([
    ['nothing', undefined],
    ['null', null],
    ['a schema without properties', { type: 'object' }],
    ['properties that are not an object', { properties: ['a'] }],
    ['something that is not a schema', 'text'],
    ['a list', [{ properties: {} }]],
  ])('takes %s as no parameters', (_name, schema) => {
    expect(toolParameters(schema)).toEqual([]);
  });

  it('ignores a malformed "required"', () => {
    expect(toolParameters({ properties: { a: { type: 'string' } }, required: 'a' })).toEqual([
      { name: 'a', type: 'string', required: false, description: '' },
    ]);
  });
});

describe('typeOf', () => {
  it.each([
    [{ type: 'array', items: { type: 'array', items: { type: 'number' } } }, 'array of array of number'],
    [{ type: 'array' }, 'array of any'],
    [undefined, 'any'],
    [42, 'any'],
    [{ type: [] }, 'any'],
  ])('%j → %s', (schema, expected) => {
    expect(typeOf(schema)).toBe(expected);
  });
});

describe('promptArguments', () => {
  it('reads the list the upstream published and skips what is not an argument', () => {
    const args: JsonValue = [
      { name: 'topic', description: 'What to summarise', required: true },
      { name: 'style' },
      { description: 'no name' },
      'text',
      null,
    ];
    expect(promptArguments(args)).toEqual([
      { name: 'topic', type: 'string', required: true, description: 'What to summarise' },
      { name: 'style', type: 'string', required: false, description: '' },
    ]);
    expect(promptArguments(undefined)).toEqual([]);
    expect(promptArguments({ name: 'x' })).toEqual([]);
  });
});

describe('text', () => {
  it('counts', () => {
    expect(count(0, 'server')).toBe('0 servers');
    expect(count(1, 'server')).toBe('1 server');
    expect(count(2, 'server')).toBe('2 servers');
    expect(count(1, 'entry', 'entries')).toBe('1 entry');
    expect(count(3, 'entry', 'entries')).toBe('3 entries');
  });

  it('shortens a long URI in the middle and leaves a short one alone', () => {
    expect(middle('trunks://res/files/a.md')).toBe('trunks://res/files/a.md');
    const long = 'trunks://res/files/ZmlsZTovLy9wcm9qZWN0cy9yZXBvcnRzLzIwMjYvcTM/summary-report.md';
    const short = middle(long);
    expect(short.length).toBeLessThanOrEqual(44);
    expect(short.startsWith('trunks://res/files/')).toBe(true);
    expect(short.endsWith('/summary-report.md')).toBe(true);
    expect(short).toContain('…');
    // a value with no useful last segment keeps its two ends
    const blob = 'trunks://res/files/' + 'Z'.repeat(80);
    expect(middle(blob).length).toBeLessThanOrEqual(44);
    expect(middle(blob).startsWith('trunks://')).toBe(true);
  });
});
