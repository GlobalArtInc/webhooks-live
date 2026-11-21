import type { Capture, ResponseConfig } from '../../api/types';
import {
  ExportError,
  curlBodyFileName,
  curlNeedsBodyFile,
  curlSkippedHeaders,
  rawBodyBlob,
  suggestedFileName,
  toCurl,
  toHar,
  toJson,
} from '../../features/export';
import type { CaptureExportInput, SkippedHeader } from '../../features/export';
import { copyText } from '../CopyButton/clipboard';
import { downloadBlob } from './download';

export type CopyAsKind = 'curl' | 'har' | 'json' | 'url';

export type CopyAsItemId = 'curl' | 'har' | 'json' | 'url' | 'downloadJson' | 'downloadRaw' | 'downloadHar';

export interface ReadyAction {
  ok: true;
  copies: CopyAsKind | null;
  run: () => Promise<boolean>;
}

export interface BlockedAction {
  ok: false;
  reason: string;
}

export type PreparedAction = ReadyAction | BlockedAction;

export type CopyAsPlan = Record<CopyAsItemId, PreparedAction>;

const JSON_INDENT = 2;
const JSON_MIME_TYPE = 'application/json';

function copyAction(text: string, kind: CopyAsKind): ReadyAction {
  return { ok: true, copies: kind, run: () => copyText(text) };
}

function downloadAction(blob: Blob, fileName: string): ReadyAction {
  return {
    ok: true,
    copies: null,
    run: () => {
      downloadBlob(blob, fileName);
      return Promise.resolve(true);
    },
  };
}

function guarded(produce: () => ReadyAction): PreparedAction {
  try {
    return produce();
  } catch (error) {
    if (error instanceof ExportError) {
      return { ok: false, reason: error.message };
    }
    throw error;
  }
}

function harText(input: CaptureExportInput, endpointResponse: ResponseConfig | undefined): string {
  return JSON.stringify(toHar(input, { endpointResponse }), null, JSON_INDENT);
}

function curlAction(input: CaptureExportInput): ReadyAction {
  const command = toCurl(input);
  if (!curlNeedsBodyFile(input)) {
    return copyAction(command, 'curl');
  }
  const body = rawBodyBlob(input);
  const fileName = curlBodyFileName(input);
  return {
    ok: true,
    copies: 'curl',
    run: async () => {
      const copied = await copyText(command);
      downloadBlob(body, fileName);
      return copied;
    },
  };
}

export function prepareCopyAs(capture: Capture, endpointResponse: ResponseConfig | undefined): CopyAsPlan {
  const input: CaptureExportInput = capture;
  return {
    curl: guarded(() => curlAction(input)),
    har: guarded(() => copyAction(harText(input, endpointResponse), 'har')),
    json: guarded(() => copyAction(toJson(input), 'json')),
    url: guarded(() => copyAction(capture.url, 'url')),
    downloadJson: guarded(() =>
      downloadAction(new Blob([toJson(input)], { type: JSON_MIME_TYPE }), suggestedFileName(input, 'json')),
    ),
    downloadRaw: guarded(() => downloadAction(rawBodyBlob(input), suggestedFileName(input, 'body'))),
    downloadHar: guarded(() =>
      downloadAction(
        new Blob([harText(input, endpointResponse)], { type: JSON_MIME_TYPE }),
        suggestedFileName(input, 'har'),
      ),
    ),
  };
}

export function unsendableHeaders(capture: Capture): SkippedHeader[] {
  const input: CaptureExportInput = capture;
  return curlSkippedHeaders(input).filter((header) => header.reason === 'unsendable');
}
