import { decodeBody } from './body';
import type { DecodedBody } from './body';
import {
  BODY_METHODS,
  DEFAULT_LARGE_BODY_FILE_THRESHOLD,
  DEFAULT_LARGE_BODY_PIPE_THRESHOLD,
  MAX_ESCAPED_CONTROL_CHARS,
} from './constants';
import { shortCaptureId } from './download';
import { ExportError } from './errors';
import { HTTP_TOKEN_PATTERN, isBlankHeaderValue, splitHeaders } from './headers';
import { countTerminalHazards, shellQuote, singleQuote, toPrintfFormat } from './shellQuote';
import type { CaptureExportInput, CurlOptions, NameValue, SkippedHeader } from './types';

const CONTINUATION = ' \\\n  ';

type BodyMode = 'none' | 'empty' | 'literal' | 'pipe' | 'file';

interface BodyPlan {
  mode: BodyMode;
  body: DecodedBody;
}

function resolveThreshold(value: number | undefined, fallback: number, label: string): number {
  if (value === undefined) {
    return fallback;
  }
  if (!Number.isInteger(value) || value < 0) {
    throw new ExportError(`${label} must be a non-negative integer`);
  }
  return value;
}

function planBody(capture: CaptureExportInput, options: CurlOptions): BodyPlan {
  const pipeThreshold = resolveThreshold(
    options.largeBodyPipeThreshold,
    DEFAULT_LARGE_BODY_PIPE_THRESHOLD,
    'largeBodyPipeThreshold',
  );
  const fileThreshold = resolveThreshold(
    options.largeBodyFileThreshold,
    DEFAULT_LARGE_BODY_FILE_THRESHOLD,
    'largeBodyFileThreshold',
  );
  const body = decodeBody(capture.body);
  const size = body.bytes.length;
  if (size === 0) {
    return { mode: BODY_METHODS.includes(capture.method) ? 'empty' : 'none', body };
  }
  if (size > fileThreshold) {
    return { mode: 'file', body };
  }
  if (body.text === null || size > pipeThreshold || countTerminalHazards(body.text) > 0) {
    return { mode: 'pipe', body };
  }
  return { mode: 'literal', body };
}

function headerArgument(header: NameValue): string {
  const line = isBlankHeaderValue(header.value) ? `${header.name};` : `${header.name}: ${header.value}`;
  return `-H ${shellQuote(line)}`;
}

export function curlBodyFileName(capture: CaptureExportInput): string {
  return `capture-${shortCaptureId(capture)}.bin`;
}

export function curlNeedsBodyFile(capture: CaptureExportInput, options: CurlOptions = {}): boolean {
  return planBody(capture, options).mode === 'file';
}

export function curlSkippedHeaders(capture: CaptureExportInput): SkippedHeader[] {
  return splitHeaders(capture.headers).skipped;
}

export function toCurl(capture: CaptureExportInput, options: CurlOptions = {}): string {
  if (!HTTP_TOKEN_PATTERN.test(capture.method)) {
    throw new ExportError('The request method is not a valid HTTP token');
  }
  if (countTerminalHazards(capture.url) > MAX_ESCAPED_CONTROL_CHARS) {
    throw new ExportError('The URL contains too many control characters');
  }
  const plan = planBody(capture, options);
  const hasBody = plan.mode !== 'none';
  const isHead = capture.method === 'HEAD' && !hasBody;
  const { sendable } = splitHeaders(capture.headers);

  const args: string[] = ['--globoff', '--path-as-is', `--url ${shellQuote(capture.url)}`];
  if (isHead) {
    args.push('--head');
  } else if (hasBody || capture.method !== 'GET') {
    args.push(`-X ${shellQuote(capture.method)}`);
  }
  for (const header of sendable) {
    args.push(headerArgument(header));
  }
  if (hasBody && !sendable.some((header) => header.name.toLowerCase() === 'content-type')) {
    args.push(`-H ${singleQuote('Content-Type:')}`);
  }

  let prefix = '';
  if (plan.mode === 'empty') {
    args.push("--data-raw ''");
  } else if (plan.mode === 'literal' && plan.body.text !== null) {
    args.push(`--data-raw ${shellQuote(plan.body.text)}`);
  } else if (plan.mode === 'pipe') {
    prefix = `printf ${singleQuote(toPrintfFormat(plan.body.bytes))} | `;
    args.push('--data-binary @-');
  } else if (plan.mode === 'file') {
    args.push(`--data-binary ${shellQuote(`@${curlBodyFileName(capture)}`)}`);
  }

  if (options.multiline === true) {
    return `${prefix}curl${CONTINUATION}${args.join(CONTINUATION)}`;
  }
  return `${prefix}curl ${args.join(' ')}`;
}
