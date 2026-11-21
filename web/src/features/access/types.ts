export type CaptureAuthKind = 'basic' | 'header';
export type AuthKindChoice = 'none' | CaptureAuthKind;
export type PasswordAction = 'keep' | 'set' | 'remove';

export interface CaptureAuthView {
  kind: CaptureAuthKind;
  name: string;
}

export interface AccessSnapshot {
  senderAllowCidrs: string[];
  captureAuth: CaptureAuthView | null;
  viewProtected: boolean;
}

export interface BasicAuthInput {
  kind: 'basic';
  username: string;
  password: string;
}

export interface HeaderAuthInput {
  kind: 'header';
  name: string;
  value: string;
}

export type CaptureAuthInput = BasicAuthInput | HeaderAuthInput;

export interface AccessPatch {
  sender_allow_cidrs?: string[];
  capture_auth?: CaptureAuthInput | null;
  view_password?: string | null;
}

export interface UnlockGrant {
  grant: string;
  expires_at: string;
}

export interface AccessDraft {
  passwordAction: PasswordAction;
  password: string;
  confirmPassword: string;
  sendersEnabled: boolean;
  cidrs: string[];
  authKind: AuthKindChoice;
  authName: string;
  authSecret: string;
}

export type IssueReason =
  | 'passwordShort'
  | 'passwordLong'
  | 'passwordMismatch'
  | 'cidrsEmpty'
  | 'authNameRequired'
  | 'authNameInvalid'
  | 'authSecretRequired'
  | 'authSecretInvalid'
  | 'notAllowed'
  | 'rejected'
  | 'unknownOutcome';

export type IssueTarget = 'password' | 'cidrs' | 'authName' | 'authSecret' | 'general';

export interface AccessIssue {
  reason: IssueReason;
  detail?: string;
}

export type AccessIssues = Partial<Record<IssueTarget, AccessIssue>>;
