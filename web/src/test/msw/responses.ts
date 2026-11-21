import { HttpResponse } from 'msw';
import type { ErrorBody, FieldError } from '../../api/types';

type HeaderMap = Record<string, string>;

export const API_HEADERS: HeaderMap = {
  'Cache-Control': 'no-store',
  'RateLimit-Limit': '60',
  'RateLimit-Remaining': '59',
};

export function jsonResponse(body: unknown, status = 200, headers: HeaderMap = {}): Response {
  return new HttpResponse(JSON.stringify(body), {
    status,
    headers: { ...API_HEADERS, ...headers, 'Content-Type': 'application/json' },
  });
}

export function emptyResponse(status = 204): Response {
  return new HttpResponse(null, { status, headers: API_HEADERS });
}

export function errorResponse(status: number, code: string, message: string, fields?: FieldError[]): Response {
  const body: ErrorBody = { error: fields === undefined ? { code, message } : { code, message, fields } };
  return jsonResponse(body, status);
}

export async function readJsonBody<T>(request: Request, fallback?: T): Promise<T | null> {
  const text = await request.text();
  if (text.trim() === '') {
    return fallback ?? null;
  }
  try {
    return JSON.parse(text) as T;
  } catch {
    return null;
  }
}
