import { describe, expect, it } from 'vitest';
import { ApiError } from '../../api/client';
import {
  accessDraftOf,
  buildAccessPatch,
  isPatchEmpty,
  issuesFromServer,
  validateAccess,
  withoutSecrets,
} from './accessDraft';
import { GENERATED_SECRET_LENGTH, generateSecret } from './secrets';
import type { AccessDraft, AccessSnapshot } from './types';

const EMPTY: AccessSnapshot = { senderAllowCidrs: [], captureAuth: null, viewProtected: false };
const WITH_AUTH: AccessSnapshot = {
  senderAllowCidrs: ['203.0.113.0/24'],
  captureAuth: { kind: 'basic', name: 'sender' },
  viewProtected: true,
};

function draftFor(snapshot: AccessSnapshot, changes: Partial<AccessDraft> = {}): AccessDraft {
  return { ...accessDraftOf(snapshot), ...changes };
}

describe('validateAccess', () => {
  it('has no issues for an untouched draft', () => {
    expect(validateAccess(draftFor(WITH_AUTH), WITH_AUTH)).toEqual({});
  });

  it('checks the password length and that both entries match', () => {
    const short = draftFor(EMPTY, { passwordAction: 'set', password: 'short', confirmPassword: 'short' });
    expect(validateAccess(short, EMPTY).password?.reason).toBe('passwordShort');
    const mismatch = draftFor(EMPTY, {
      passwordAction: 'set',
      password: 'placeholder-1',
      confirmPassword: 'other-1234',
    });
    expect(validateAccess(mismatch, EMPTY).password?.reason).toBe('passwordMismatch');
    const long = draftFor(EMPTY, {
      passwordAction: 'set',
      password: 'x'.repeat(129),
      confirmPassword: 'x'.repeat(129),
    });
    expect(validateAccess(long, EMPTY).password?.reason).toBe('passwordLong');
    const fine = draftFor(EMPTY, {
      passwordAction: 'set',
      password: 'placeholder-1',
      confirmPassword: 'placeholder-1',
    });
    expect(validateAccess(fine, EMPTY)).toEqual({});
  });

  it('refuses an enabled allow list without entries', () => {
    expect(validateAccess(draftFor(EMPTY, { sendersEnabled: true }), EMPTY).cidrs?.reason).toBe('cidrsEmpty');
  });

  it('needs the secret again when the method or the name changes, but not when nothing changed', () => {
    const renamed = draftFor(WITH_AUTH, { authName: 'other' });
    expect(validateAccess(renamed, WITH_AUTH).authSecret?.reason).toBe('authSecretRequired');
    const switched = draftFor(EMPTY, { authKind: 'header', authName: 'X-Test-Token' });
    expect(validateAccess(switched, EMPTY).authSecret?.reason).toBe('authSecretRequired');
    expect(validateAccess(draftFor(WITH_AUTH), WITH_AUTH).authSecret).toBeUndefined();
  });

  it('rejects colons in a user name, bad header names, and non-ASCII secrets', () => {
    const colon = draftFor(EMPTY, { authKind: 'basic', authName: 'a:b', authSecret: 'placeholder-secret-1' });
    expect(validateAccess(colon, EMPTY).authName?.reason).toBe('authNameInvalid');
    const header = draftFor(EMPTY, { authKind: 'header', authName: 'not a token', authSecret: 'placeholder-secret-1' });
    expect(validateAccess(header, EMPTY).authName?.reason).toBe('authNameInvalid');
    const accent = draftFor(EMPTY, { authKind: 'header', authName: 'X-Test', authSecret: 'sécret' });
    expect(validateAccess(accent, EMPTY).authSecret?.reason).toBe('authSecretInvalid');
  });
});

describe('buildAccessPatch', () => {
  it('is empty for an untouched draft', () => {
    expect(isPatchEmpty(buildAccessPatch(draftFor(WITH_AUTH), WITH_AUTH))).toBe(true);
  });

  it('sends the password, null to remove it, and nothing otherwise', () => {
    const set = draftFor(EMPTY, { passwordAction: 'set', password: 'placeholder-1', confirmPassword: 'placeholder-1' });
    expect(buildAccessPatch(set, EMPTY)).toEqual({ view_password: 'placeholder-1' });
    expect(buildAccessPatch(draftFor(WITH_AUTH, { passwordAction: 'remove' }), WITH_AUTH)).toEqual({
      view_password: null,
    });
  });

  it('sends an empty list when the allow list is switched off and the new list when it changes', () => {
    expect(buildAccessPatch(draftFor(WITH_AUTH, { sendersEnabled: false }), WITH_AUTH)).toEqual({
      sender_allow_cidrs: [],
    });
    expect(buildAccessPatch(draftFor(WITH_AUTH, { cidrs: ['198.51.100.4/32'] }), WITH_AUTH)).toEqual({
      sender_allow_cidrs: ['198.51.100.4/32'],
    });
  });

  it('builds the capture authentication object for each method and null to remove it', () => {
    const basic = draftFor(EMPTY, { authKind: 'basic', authName: 'sender', authSecret: 'placeholder-secret-1' });
    expect(buildAccessPatch(basic, EMPTY)).toEqual({
      capture_auth: { kind: 'basic', username: 'sender', password: 'placeholder-secret-1' },
    });
    const header = draftFor(EMPTY, { authKind: 'header', authName: 'X-Test', authSecret: 'placeholder-secret-1' });
    expect(buildAccessPatch(header, EMPTY)).toEqual({
      capture_auth: { kind: 'header', name: 'X-Test', value: 'placeholder-secret-1' },
    });
    expect(buildAccessPatch(draftFor(WITH_AUTH, { authKind: 'none', authName: '' }), WITH_AUTH)).toEqual({
      capture_auth: null,
    });
  });
});

describe('withoutSecrets and generateSecret', () => {
  it('clears every secret field and keeps the rest', () => {
    const draft = draftFor(EMPTY, {
      passwordAction: 'set',
      password: 'placeholder-1',
      confirmPassword: 'placeholder-1',
      authKind: 'header',
      authName: 'X-Test',
      authSecret: 'placeholder-secret-1',
    });
    expect(withoutSecrets(draft)).toMatchObject({
      passwordAction: 'set',
      password: '',
      confirmPassword: '',
      authName: 'X-Test',
      authSecret: '',
    });
  });

  it('generates a url-safe secret of the fixed length from the supplied bytes', () => {
    const secret = generateSecret((length) => Uint8Array.from({ length }, (_, index) => index * 7));
    expect(secret).toHaveLength(GENERATED_SECRET_LENGTH);
    expect(secret).toMatch(/^[A-Za-z0-9_-]+$/);
    expect(generateSecret()).not.toBe(generateSecret());
  });
});

describe('issuesFromServer', () => {
  function validation(field: string, code: string): ApiError {
    return new ApiError({
      status: 422,
      code: 'validation_failed',
      message: `${field} refused`,
      details: [{ field, code, message: 'refused' }],
      retryAfter: null,
    });
  }

  it('maps field errors to the form fields', () => {
    expect(issuesFromServer(validation('view_password', 'not_allowed')).password?.reason).toBe('notAllowed');
    expect(issuesFromServer(validation('sender_allow_cidrs.2', 'invalid_format')).cidrs?.reason).toBe('rejected');
    expect(issuesFromServer(validation('capture_auth.password', 'too_long')).authSecret?.reason).toBe('rejected');
    expect(issuesFromServer(validation('capture_auth.username', 'invalid_format')).authName?.reason).toBe('rejected');
  });

  it('ignores errors that are not field validation', () => {
    expect(issuesFromServer(new TypeError('offline'))).toEqual({});
  });
});
