import { API_PREFIX } from '../../api/client';

export class PartRequestError extends Error {
  readonly status: number | null;

  constructor(message: string, status: number | null) {
    super(message);
    this.name = 'PartRequestError';
    this.status = status;
  }
}

export interface PartFetchOptions {
  view?: 'text';
  maxBytes: number;
  signal?: AbortSignal;
}

export interface PartLinkOptions {
  download?: boolean;
}

function assertIndex(index: number): void {
  if (!Number.isSafeInteger(index) || index < 0) {
    throw new PartRequestError('Invalid part index', null);
  }
}

function partPath(endpointId: string, captureId: string, index: number): string {
  assertIndex(index);
  return `${API_PREFIX}/endpoints/${encodeURIComponent(endpointId)}/captures/${encodeURIComponent(captureId)}/parts/${index}`;
}

export function partHref(endpointId: string, captureId: string, index: number, options: PartLinkOptions = {}): string {
  const query = options.download === true ? '?download=1' : '';
  return `${partPath(endpointId, captureId, index)}${query}`;
}

export async function fetchPartBlob(
  endpointId: string,
  captureId: string,
  index: number,
  options: PartFetchOptions,
): Promise<Blob> {
  const query = options.view === undefined ? '' : `?view=${options.view}`;
  const controller = new AbortController();
  const signal =
    options.signal === undefined ? controller.signal : AbortSignal.any([options.signal, controller.signal]);
  const response = await fetch(`${partPath(endpointId, captureId, index)}${query}`, {
    method: 'GET',
    credentials: 'same-origin',
    signal,
  });
  if (!response.ok) {
    throw new PartRequestError(`Part request failed with status ${response.status}`, response.status);
  }
  const declared = Number(response.headers.get('Content-Length'));
  if (Number.isFinite(declared) && declared > options.maxBytes) {
    controller.abort();
    throw tooLarge(response.status);
  }
  if (response.body === null) {
    return new Blob([]);
  }
  const reader = response.body.getReader();
  const chunks: Uint8Array<ArrayBuffer>[] = [];
  let total = 0;
  for (;;) {
    const step = await reader.read();
    if (step.done) {
      break;
    }
    total += step.value.byteLength;
    if (total > options.maxBytes) {
      controller.abort();
      throw tooLarge(response.status);
    }
    chunks.push(step.value);
  }
  return new Blob(chunks);
}

function tooLarge(status: number): PartRequestError {
  return new PartRequestError('The part is larger than the allowed size', status);
}
