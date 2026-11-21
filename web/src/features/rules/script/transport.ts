import { API_PREFIX, ApiError } from '../../../api/client';
import { SCRIPT_META_PATH, SCRIPT_VALIDATE_PATH } from './constants';
import { parseScriptDiagnostics, parseScriptMeta } from './parse';
import type { ScriptDiagnostics, ScriptLanguageMeta, ScriptTransport } from './types';

interface ErrorFields {
  code: string | null;
  message: string | null;
}

function readErrorFields(text: string): ErrorFields {
  let parsed: unknown;
  try {
    parsed = JSON.parse(text);
  } catch {
    return { code: null, message: null };
  }
  if (typeof parsed !== 'object' || parsed === null || !('error' in parsed)) {
    return { code: null, message: null };
  }
  const error = parsed.error;
  if (typeof error !== 'object' || error === null) {
    return { code: null, message: null };
  }
  return {
    code: 'code' in error && typeof error.code === 'string' ? error.code : null,
    message: 'message' in error && typeof error.message === 'string' ? error.message : null,
  };
}

async function failure(response: Response): Promise<ApiError> {
  const text = await response.text().catch(() => '');
  const fields = readErrorFields(text);
  return new ApiError({
    status: response.status,
    code: fields.code,
    message: fields.message ?? `Request failed with status ${response.status}`,
    details: [],
    retryAfter: null,
  });
}

async function call(path: string, init: RequestInit): Promise<unknown> {
  const url = new URL(`${API_PREFIX}${path}`, window.location.origin);
  const response = await fetch(url, { ...init, credentials: 'same-origin' });
  if (!response.ok) {
    throw await failure(response);
  }
  return response.json();
}

async function loadMeta(signal: AbortSignal): Promise<ScriptLanguageMeta> {
  return parseScriptMeta(await call(SCRIPT_META_PATH, { method: 'GET', signal }));
}

async function validate(source: string, signal: AbortSignal): Promise<ScriptDiagnostics> {
  const payload = await call(SCRIPT_VALIDATE_PATH, {
    method: 'POST',
    headers: { 'Content-Type': 'application/json' },
    body: JSON.stringify({ source }),
    signal,
  });
  return parseScriptDiagnostics(payload);
}

export const httpScriptTransport: ScriptTransport = { id: 'http', loadMeta, validate };
