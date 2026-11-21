import type { DiagnosticSeverity, EditorDiagnostic } from '../../editor/types';
import { MAX_CATALOGUE_ENTRIES, MAX_DIAGNOSTICS, MAX_KEYWORDS } from './constants';
import type {
  ScriptDiagnostics,
  ScriptFunctionDoc,
  ScriptLanguageMeta,
  ScriptLimits,
  ScriptPhaseLimits,
  ScriptVariableDoc,
} from './types';

type UnknownRecord = Record<string, unknown>;

export class ScriptContractError extends Error {
  constructor(message: string) {
    super(message);
    this.name = 'ScriptContractError';
  }
}

const NAME_PATTERN = /^[A-Za-z_][A-Za-z0-9_]*(?:\.[A-Za-z_][A-Za-z0-9_]*)*$/;
const KEYWORD_PATTERN = /^[A-Za-z_][A-Za-z0-9_]*$/;
const SAFE_REFERENCE = /^(?:\/(?!\/)|https:\/\/)\S*$/;

function isRecord(value: unknown): value is UnknownRecord {
  return typeof value === 'object' && value !== null && !Array.isArray(value);
}

function readRecord(value: unknown, where: string): UnknownRecord {
  if (!isRecord(value)) {
    throw new ScriptContractError(`${where} is not an object`);
  }
  return value;
}

function readString(record: UnknownRecord, field: string, where: string): string {
  const value = record[field];
  if (typeof value !== 'string') {
    throw new ScriptContractError(`${where}.${field} is not a string`);
  }
  return value;
}

function readPositiveInteger(record: UnknownRecord, field: string, where: string): number {
  const value = record[field];
  if (typeof value !== 'number' || !Number.isInteger(value) || value < 1) {
    throw new ScriptContractError(`${where}.${field} is not a positive integer`);
  }
  return value;
}

function readArray(record: UnknownRecord, field: string, where: string, max: number): unknown[] {
  const value = record[field];
  if (!Array.isArray(value)) {
    throw new ScriptContractError(`${where}.${field} is not a list`);
  }
  if (value.length > max) {
    throw new ScriptContractError(`${where}.${field} has more than ${max} entries`);
  }
  return value;
}

function readName(record: UnknownRecord, where: string): string {
  const name = readString(record, 'name', where);
  if (!NAME_PATTERN.test(name)) {
    throw new ScriptContractError(`${where}.name is not an identifier path`);
  }
  return name;
}

function parseFunction(value: unknown, index: number): ScriptFunctionDoc {
  const where = `functions[${index}]`;
  const record = readRecord(value, where);
  return {
    name: readName(record, where),
    signature: readString(record, 'signature', where),
    doc: readString(record, 'doc', where),
  };
}

function parseVariable(value: unknown, index: number): ScriptVariableDoc {
  const where = `variables[${index}]`;
  const record = readRecord(value, where);
  return {
    name: readName(record, where),
    type: readString(record, 'type', where),
    doc: readString(record, 'doc', where),
  };
}

function parsePhaseLimits(value: unknown, where: string): ScriptPhaseLimits {
  const record = readRecord(value, where);
  return {
    max_operations: readPositiveInteger(record, 'max_operations', where),
    max_time_ms: readPositiveInteger(record, 'max_time_ms', where),
  };
}

function parseLimits(value: unknown): ScriptLimits {
  const record = readRecord(value, 'limits');
  return {
    max_source_bytes: readPositiveInteger(record, 'max_source_bytes', 'limits'),
    before_response: parsePhaseLimits(record.before_response, 'limits.before_response'),
    after_response: parsePhaseLimits(record.after_response, 'limits.after_response'),
  };
}

function parseKeyword(value: unknown, index: number): string {
  if (typeof value !== 'string' || !KEYWORD_PATTERN.test(value)) {
    throw new ScriptContractError(`keywords[${index}] is not a word`);
  }
  return value;
}

function parseReference(value: unknown): string | null {
  if (value === null || value === undefined) {
    return null;
  }
  if (typeof value !== 'string' || !SAFE_REFERENCE.test(value)) {
    throw new ScriptContractError('reference_url is neither an absolute path nor an https address');
  }
  return value;
}

export function parseScriptMeta(payload: unknown): ScriptLanguageMeta {
  const record = readRecord(payload, 'script meta');
  return {
    name: readString(record, 'name', 'meta'),
    keywords: readArray(record, 'keywords', 'meta', MAX_KEYWORDS).map(parseKeyword),
    functions: readArray(record, 'functions', 'meta', MAX_CATALOGUE_ENTRIES).map(parseFunction),
    variables: readArray(record, 'variables', 'meta', MAX_CATALOGUE_ENTRIES).map(parseVariable),
    limits: parseLimits(record.limits),
    reference_url: parseReference(record.reference_url),
  };
}

function parseSeverity(record: UnknownRecord, where: string): DiagnosticSeverity {
  const value = record.severity;
  if (value === undefined) {
    return 'error';
  }
  if (value === 'error' || value === 'warning') {
    return value;
  }
  throw new ScriptContractError(`${where}.severity is neither error nor warning`);
}

function parseDiagnostic(value: unknown, index: number): EditorDiagnostic {
  const where = `diagnostics[${index}]`;
  const record = readRecord(value, where);
  return {
    line: readPositiveInteger(record, 'line', where),
    column: readPositiveInteger(record, 'column', where),
    length: readPositiveInteger(record, 'length', where),
    code: readString(record, 'code', where),
    message: readString(record, 'message', where),
    severity: parseSeverity(record, where),
  };
}

export function parseScriptDiagnostics(payload: unknown): ScriptDiagnostics {
  const record = readRecord(payload, 'validation answer');
  const items = readArray(record, 'diagnostics', 'answer', Number.MAX_SAFE_INTEGER);
  return items.slice(0, MAX_DIAGNOSTICS).map(parseDiagnostic);
}
