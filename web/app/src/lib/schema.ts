// Reads what a tool or a prompt takes as input from the JSON the upstream published. The
// gateway passes that JSON through untouched, so nothing about its shape is assumed.

import type { JsonValue } from '../api/types.gen';

export interface Parameter {
  name: string;
  type: string;
  required: boolean;
  description: string;
}

type JsonObject = { [key: string]: JsonValue };

function object(value: JsonValue | undefined | null): JsonObject | undefined {
  return typeof value === 'object' && value !== null && !Array.isArray(value) ? value : undefined;
}

function text(value: JsonValue | undefined): string {
  return typeof value === 'string' ? value : '';
}

/** `string`, `array of string`, `string or null`, `object`… from a JSON Schema fragment. */
export function typeOf(schema: JsonValue | undefined): string {
  const s = object(schema);
  if (s === undefined) return 'any';
  const type = s['type'];
  if (Array.isArray(type)) return type.map((t) => text(t)).filter(Boolean).join(' or ') || 'any';
  if (type === 'array') return `array of ${typeOf(s['items'])}`;
  if (typeof type === 'string') return type;
  const variants = s['anyOf'] ?? s['oneOf'];
  if (Array.isArray(variants)) return variants.map((v) => typeOf(v)).join(' or ');
  if (Array.isArray(s['enum'])) return 'enum';
  return 'any';
}

/** The input parameters of a tool, from its JSON Schema (`properties` and `required`). */
export function toolParameters(inputSchema: JsonValue | undefined | null): Parameter[] {
  const properties = object(object(inputSchema)?.['properties']);
  if (properties === undefined) return [];
  const required = object(inputSchema)?.['required'];
  const names = new Set(Array.isArray(required) ? required.map((r) => text(r)) : []);
  return Object.entries(properties).map(([name, schema]) => ({
    name,
    type: typeOf(schema),
    required: names.has(name),
    description: text(object(schema)?.['description']),
  }));
}

/** The arguments of a prompt: a list of `{name, description?, required?}`. */
export function promptArguments(args: JsonValue | undefined | null): Parameter[] {
  if (!Array.isArray(args)) return [];
  return args.flatMap((arg) => {
    const a = object(arg);
    const name = text(a?.['name']);
    if (a === undefined || name === '') return [];
    return [{ name, type: 'string', required: a['required'] === true, description: text(a['description']) }];
  });
}
