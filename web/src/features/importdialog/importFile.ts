export const IMPORT_MAX_BYTES = 100 * 1024 * 1024;
export const PREVIEW_BYTES = 64 * 1024;
export const EXPORT_FORMAT_NAME = 'webhooks-live-export';

const BYTE_ORDER_MARK = '﻿';
const NEWLINE = '\n';
const ENDPOINT_KEY = '"endpoint"';
const VERSION_PATTERN = /"version"\s*:\s*(\d{1,6})\b/u;
const EXPORTED_PATTERN = /"exported_at"\s*:\s*"((?:[^"\\]|\\.){0,64})"/u;
const TITLE_PATTERN = /"title"\s*:\s*"((?:[^"\\]|\\.){0,512})"/u;

export type ImportFormat = 'json' | 'ndjson';

export type FileProblem = 'zip' | 'extension' | 'empty' | 'tooLarge' | 'notExport' | 'unreadable';

export type UnknownRecord = Record<string, unknown>;

export interface FileFacts {
  name: string;
  size: number;
}

export interface ExportPreview {
  format: ImportFormat;
  version: number | null;
  endpointTitle: string | null;
  exportedAt: string | null;
}

export interface AcceptedFile {
  ok: true;
  preview: ExportPreview;
}

export interface RejectedFile {
  ok: false;
  problem: FileProblem;
}

export type FileInspection = AcceptedFile | RejectedFile;

export interface ChosenFile {
  file: File;
  inspection: FileInspection | null;
}

export function isRecord(value: unknown): value is UnknownRecord {
  return typeof value === 'object' && value !== null && !Array.isArray(value);
}

export function parseRecord(text: string): UnknownRecord | null {
  try {
    const value: unknown = JSON.parse(text);
    return isRecord(value) ? value : null;
  } catch {
    return null;
  }
}

export function precheck(facts: FileFacts): FileProblem | null {
  const name = facts.name.toLowerCase();
  if (name.endsWith('.zip')) {
    return 'zip';
  }
  if (!name.endsWith('.json') && !name.endsWith('.ndjson')) {
    return 'extension';
  }
  if (facts.size === 0) {
    return 'empty';
  }
  if (facts.size > IMPORT_MAX_BYTES) {
    return 'tooLarge';
  }
  return null;
}

export function readSample(file: Blob): Promise<string> {
  return new Promise<string>((resolve, reject) => {
    const reader = new FileReader();
    reader.onload = () => {
      resolve(typeof reader.result === 'string' ? reader.result : '');
    };
    reader.onerror = () => {
      reject(reader.error ?? new Error('The file could not be read.'));
    };
    reader.readAsText(file.slice(0, PREVIEW_BYTES));
  });
}

export async function inspectFile(file: File): Promise<FileInspection> {
  const problem = precheck(file);
  if (problem !== null) {
    return { ok: false, problem };
  }
  try {
    return inspectSample(await readSample(file));
  } catch {
    return { ok: false, problem: 'unreadable' };
  }
}

export function inspectSample(sample: string): FileInspection {
  const text = sample.startsWith(BYTE_ORDER_MARK) ? sample.slice(BYTE_ORDER_MARK.length) : sample;
  const trimmed = text.trimStart();
  if (trimmed === '') {
    return { ok: false, problem: 'empty' };
  }
  if (!trimmed.startsWith('{')) {
    return { ok: false, problem: 'notExport' };
  }
  const firstLine = trimmed.split(NEWLINE, 1)[0] ?? '';
  return { ok: true, preview: headerPreview(firstLine) ?? documentPreview(trimmed) };
}

function headerPreview(line: string): ExportPreview | null {
  const value = parseRecord(line);
  if (value === null || 'captures' in value || value.format !== EXPORT_FORMAT_NAME) {
    return null;
  }
  return {
    format: 'ndjson',
    version: numberOf(value.version),
    endpointTitle: isRecord(value.endpoint) ? textOf(value.endpoint.title) : null,
    exportedAt: textOf(value.exported_at),
  };
}

function documentPreview(text: string): ExportPreview {
  const endpointAt = text.indexOf(ENDPOINT_KEY);
  const fromEndpoint = endpointAt === -1 ? '' : text.slice(endpointAt);
  const digits = VERSION_PATTERN.exec(text)?.[1];
  return {
    format: 'json',
    version: digits === undefined ? null : Number(digits),
    endpointTitle: decodeText(TITLE_PATTERN.exec(fromEndpoint)?.[1] ?? null),
    exportedAt: decodeText(EXPORTED_PATTERN.exec(text)?.[1] ?? null),
  };
}

function decodeText(raw: string | null): string | null {
  if (raw === null || raw === '') {
    return null;
  }
  try {
    const value: unknown = JSON.parse(`"${raw}"`);
    return typeof value === 'string' && value !== '' ? value : null;
  } catch {
    return raw;
  }
}

function numberOf(value: unknown): number | null {
  return typeof value === 'number' && Number.isInteger(value) ? value : null;
}

function textOf(value: unknown): string | null {
  return typeof value === 'string' && value !== '' ? value : null;
}
