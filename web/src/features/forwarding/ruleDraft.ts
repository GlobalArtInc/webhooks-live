import type { FieldError, ForwardRule, ForwardRuleInput } from '../../api/types';

export const TARGET_MAX_CHARS = 2048;
export const TARGET_FIELD = 'target_url';
const HTTP_PROTOCOLS: readonly string[] = ['http:', 'https:'];

export interface RuleDraft {
  targetUrl: string;
  method: string;
  contentType: string;
  headers: string;
  appendPath: boolean;
}

export type RuleIssueCode = 'required' | 'invalid' | 'tooLong' | 'targetRefused' | 'targetUnresolvable';

export interface RuleIssue {
  code: RuleIssueCode;
  detail: string | null;
}

export function draftFromRule(rule: ForwardRule | null): RuleDraft {
  if (rule === null) {
    return { targetUrl: '', method: '', contentType: '', headers: '', appendPath: false };
  }
  return {
    targetUrl: rule.target_url,
    method: rule.method ?? '',
    contentType: rule.content_type ?? '',
    headers: rule.header_allowlist.join(', '),
    appendPath: rule.append_path,
  };
}

export function splitHeaderNames(text: string): string[] {
  return text
    .split(',')
    .map((name) => name.trim())
    .filter((name) => name !== '');
}

function optionalText(text: string): string | null {
  const value = text.trim();
  return value === '' ? null : value;
}

export function ruleInputFromDraft(draft: RuleDraft, enabled: boolean): ForwardRuleInput {
  return {
    enabled,
    target_url: draft.targetUrl.trim(),
    method: optionalText(draft.method),
    content_type: optionalText(draft.contentType),
    header_allowlist: splitHeaderNames(draft.headers),
    append_path: draft.appendPath,
  };
}

export function validateTarget(text: string): RuleIssue | null {
  const value = text.trim();
  if (value === '') {
    return { code: 'required', detail: null };
  }
  if (value.length > TARGET_MAX_CHARS) {
    return { code: 'tooLong', detail: null };
  }
  try {
    return HTTP_PROTOCOLS.includes(new URL(value).protocol) ? null : { code: 'invalid', detail: null };
  } catch {
    return { code: 'invalid', detail: null };
  }
}

export function issueFromServer(fields: readonly FieldError[]): RuleIssue | null {
  const field = fields.find((item) => item.field === TARGET_FIELD);
  if (field === undefined) {
    return null;
  }
  switch (field.code) {
    case 'target_refused':
      return { code: 'targetRefused', detail: field.message };
    case 'target_unresolvable':
      return { code: 'targetUnresolvable', detail: null };
    case 'too_long':
      return { code: 'tooLong', detail: null };
    default:
      return { code: 'invalid', detail: null };
  }
}
