import type { Endpoint } from '../../api/types';
import type { AccessSnapshot, CaptureAuthView } from './types';

type UnknownRecord = Record<string, unknown>;

function isRecord(value: unknown): value is UnknownRecord {
  return typeof value === 'object' && value !== null && !Array.isArray(value);
}

function readCidrs(value: unknown): string[] {
  return Array.isArray(value) ? value.filter((item): item is string => typeof item === 'string') : [];
}

function readCaptureAuth(value: unknown): CaptureAuthView | null {
  if (!isRecord(value)) {
    return null;
  }
  if (value.kind === 'basic' && typeof value.username === 'string') {
    return { kind: 'basic', name: value.username };
  }
  if (value.kind === 'header' && typeof value.name === 'string') {
    return { kind: 'header', name: value.name };
  }
  return null;
}

export function readAccess(endpoint: Endpoint): AccessSnapshot {
  const raw: unknown = endpoint;
  const fields: UnknownRecord = isRecord(raw) ? raw : {};
  return {
    senderAllowCidrs: readCidrs(fields.sender_allow_cidrs),
    captureAuth: readCaptureAuth(fields.capture_auth),
    viewProtected: fields.view_protected === true,
  };
}

export function readClientIp(instance: unknown): string | null {
  if (!isRecord(instance)) {
    return null;
  }
  const value = instance.client_ip;
  return typeof value === 'string' && value !== '' ? value : null;
}
