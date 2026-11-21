import { ApiError } from '../../api/client';
import { PASSWORD_MAX, PASSWORD_MIN, SECRET_MAX_BYTES, isHeaderName, isVisibleAscii } from './secrets';
import type {
  AccessDraft,
  AccessIssue,
  AccessIssues,
  AccessPatch,
  AccessSnapshot,
  AuthKindChoice,
  CaptureAuthInput,
  IssueTarget,
} from './types';

const USERNAME_SEPARATOR = ':';
const NO_ISSUES: AccessIssues = {};

export function accessDraftOf(snapshot: AccessSnapshot): AccessDraft {
  return {
    passwordAction: 'keep',
    password: '',
    confirmPassword: '',
    sendersEnabled: snapshot.senderAllowCidrs.length > 0,
    cidrs: snapshot.senderAllowCidrs,
    authKind: snapshot.captureAuth?.kind ?? 'none',
    authName: snapshot.captureAuth?.name ?? '',
    authSecret: '',
  };
}

export function serializeAccessDraft(draft: AccessDraft): string {
  return JSON.stringify(draft);
}

export function withoutSecrets(draft: AccessDraft): AccessDraft {
  return { ...draft, password: '', confirmPassword: '', authSecret: '' };
}

export function authChanged(draft: AccessDraft, base: AccessSnapshot): boolean {
  const baseKind: AuthKindChoice = base.captureAuth?.kind ?? 'none';
  if (draft.authKind !== baseKind) {
    return true;
  }
  return draft.authKind !== 'none' && (draft.authName !== base.captureAuth?.name || draft.authSecret !== '');
}

function passwordIssue(draft: AccessDraft): AccessIssue | null {
  if (draft.passwordAction !== 'set') {
    return null;
  }
  if (draft.password.length < PASSWORD_MIN) {
    return { reason: 'passwordShort' };
  }
  if (draft.password.length > PASSWORD_MAX) {
    return { reason: 'passwordLong' };
  }
  return draft.password === draft.confirmPassword ? null : { reason: 'passwordMismatch' };
}

function nameIssue(draft: AccessDraft): AccessIssue | null {
  const name = draft.authName;
  if (name === '') {
    return { reason: 'authNameRequired' };
  }
  const valid =
    name.length <= SECRET_MAX_BYTES &&
    (draft.authKind === 'header' ? isHeaderName(name) : isVisibleAscii(name) && !name.includes(USERNAME_SEPARATOR));
  return valid ? null : { reason: 'authNameInvalid' };
}

function secretIssue(draft: AccessDraft): AccessIssue | null {
  if (draft.authSecret === '') {
    return { reason: 'authSecretRequired' };
  }
  const valid = draft.authSecret.length <= SECRET_MAX_BYTES && isVisibleAscii(draft.authSecret);
  return valid ? null : { reason: 'authSecretInvalid' };
}

export function validateAccess(draft: AccessDraft, base: AccessSnapshot): AccessIssues {
  const issues: AccessIssues = {};
  const password = passwordIssue(draft);
  if (password !== null) {
    issues.password = password;
  }
  if (draft.sendersEnabled && draft.cidrs.length === 0) {
    issues.cidrs = { reason: 'cidrsEmpty' };
  }
  if (draft.authKind !== 'none' && authChanged(draft, base)) {
    const name = nameIssue(draft);
    const secret = secretIssue(draft);
    if (name !== null) {
      issues.authName = name;
    }
    if (secret !== null) {
      issues.authSecret = secret;
    }
  }
  return issues;
}

export function isAccessValid(draft: AccessDraft, base: AccessSnapshot): boolean {
  return Object.keys(validateAccess(draft, base)).length === 0;
}

function authInputOf(draft: AccessDraft): CaptureAuthInput | null {
  switch (draft.authKind) {
    case 'basic':
      return { kind: 'basic', username: draft.authName, password: draft.authSecret };
    case 'header':
      return { kind: 'header', name: draft.authName, value: draft.authSecret };
    case 'none':
      return null;
  }
}

export function buildAccessPatch(draft: AccessDraft, base: AccessSnapshot): AccessPatch {
  const patch: AccessPatch = {};
  if (draft.passwordAction === 'set') {
    patch.view_password = draft.password;
  } else if (draft.passwordAction === 'remove') {
    patch.view_password = null;
  }
  const cidrs = draft.sendersEnabled ? draft.cidrs : [];
  if (JSON.stringify(cidrs) !== JSON.stringify(base.senderAllowCidrs)) {
    patch.sender_allow_cidrs = cidrs;
  }
  if (authChanged(draft, base)) {
    patch.capture_auth = authInputOf(draft);
  }
  return patch;
}

export function isPatchEmpty(patch: AccessPatch): boolean {
  return Object.keys(patch).length === 0;
}

interface ServerIssue {
  target: IssueTarget;
  issue: AccessIssue;
}

function targetOf(field: string): IssueTarget {
  if (field === 'view_password') {
    return 'password';
  }
  if (field.startsWith('sender_allow_cidrs')) {
    return 'cidrs';
  }
  if (field === 'capture_auth.username' || field === 'capture_auth.name') {
    return 'authName';
  }
  return field.startsWith('capture_auth') ? 'authSecret' : 'general';
}

function serverIssueOf(field: string, code: string | undefined, message: string): ServerIssue {
  const notAllowed = code === 'not_allowed';
  const issue: AccessIssue = notAllowed ? { reason: 'notAllowed' } : { reason: 'rejected', detail: message };
  return { target: targetOf(field), issue };
}

export function issuesFromServer(error: unknown): AccessIssues {
  if (!(error instanceof ApiError) || error.status !== 422 || error.details.length === 0) {
    return NO_ISSUES;
  }
  const issues: AccessIssues = {};
  for (const field of error.details) {
    const { target, issue } = serverIssueOf(field.field, field.code, field.message);
    if (issues[target] === undefined) {
      issues[target] = issue;
    }
  }
  return issues;
}
