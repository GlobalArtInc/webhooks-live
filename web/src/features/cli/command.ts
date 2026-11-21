import { CLI_BINARY } from './release';

export const DEFAULT_PORT = 3000;

const MIN_PORT = 1;
const MAX_PORT = 65535;
const PORT_DIGITS = /^\d{1,5}$/;
const PREFIX_PATTERN = /^(\/[A-Za-z0-9._~/-]*)?$/;
const SAFE_WORD = /^[A-Za-z0-9._~:/@%+=,-]+$/;

export type StartPoint = 'resume' | 'now' | 'all';

export interface ForwardCommandOptions {
  endpointUrl: string;
  port: number;
  prefix: string;
  start: StartPoint;
  dryRun: boolean;
}

export function parsePort(text: string): number | null {
  const trimmed = text.trim();
  if (!PORT_DIGITS.test(trimmed)) {
    return null;
  }
  const port = Number(trimmed);
  return port >= MIN_PORT && port <= MAX_PORT ? port : null;
}

export function isValidPrefix(prefix: string): boolean {
  return PREFIX_PATTERN.test(prefix);
}

export function localTargetUrl(port: number, prefix: string): string {
  return `http://localhost:${port}${prefix}`;
}

export function shellWord(value: string): string {
  return SAFE_WORD.test(value) ? value : `'${value.replace(/'/g, `'\\''`)}'`;
}

export function buildForwardCommand(options: ForwardCommandOptions): string {
  const words = [
    CLI_BINARY,
    'forward',
    shellWord(options.endpointUrl),
    '--to',
    localTargetUrl(options.port, options.prefix),
  ];
  if (options.start !== 'resume') {
    words.push('--from', options.start);
  }
  if (options.dryRun) {
    words.push('--dry-run');
  }
  return words.join(' ');
}
