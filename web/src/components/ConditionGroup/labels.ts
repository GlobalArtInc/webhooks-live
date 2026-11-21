import type { TFunction } from 'i18next';
import type { OperatorEntry, ParameterKind, RowIssue, SubjectEntry } from '../../features/rules/conditions/types';

export interface CatalogueLabel {
  text: string;
  fallback: boolean;
}

function translated(text: string): CatalogueLabel {
  return { text, fallback: false };
}

export function operatorLabel(t: TFunction, operator: OperatorEntry): CatalogueLabel {
  switch (operator.id) {
    case 'eq':
      return translated(t('wl.conditions.operator.eq'));
    case 'ne':
      return translated(t('wl.conditions.operator.ne'));
    case 'contains':
      return translated(t('wl.conditions.operator.contains'));
    case 'starts_with':
      return translated(t('wl.conditions.operator.startsWith'));
    case 'ends_with':
      return translated(t('wl.conditions.operator.endsWith'));
    case 'matches':
      return translated(t('wl.conditions.operator.matches'));
    case 'exists':
      return translated(t('wl.conditions.operator.exists'));
    case 'gt':
      return translated(t('wl.conditions.operator.gt'));
    case 'lt':
      return translated(t('wl.conditions.operator.lt'));
    case 'in':
      return translated(t('wl.conditions.operator.in'));
    case 'in_cidr':
      return translated(t('wl.conditions.operator.inCidr'));
    default:
      return { text: operator.label, fallback: true };
  }
}

export function subjectLabel(t: TFunction, subject: SubjectEntry): CatalogueLabel {
  switch (subject.id) {
    case 'method':
      return translated(t('wl.conditions.subject.method'));
    case 'path':
      return translated(t('wl.conditions.subject.path'));
    case 'query':
      return translated(t('wl.conditions.subject.query'));
    case 'header':
      return translated(t('wl.conditions.subject.header'));
    case 'json':
      return translated(t('wl.conditions.subject.json'));
    case 'body':
      return translated(t('wl.conditions.subject.body'));
    case 'form':
      return translated(t('wl.conditions.subject.form'));
    case 'sender':
      return translated(t('wl.conditions.subject.sender'));
    case 'email_from':
      return translated(t('wl.conditions.subject.emailFrom'));
    case 'email_to':
      return translated(t('wl.conditions.subject.emailTo'));
    case 'email_subject':
      return translated(t('wl.conditions.subject.emailSubject'));
    case 'dns_name':
      return translated(t('wl.conditions.subject.dnsName'));
    case 'dns_type':
      return translated(t('wl.conditions.subject.dnsType'));
    case 'vars':
      return translated(t('wl.conditions.subject.vars'));
    case 'variable':
      return translated(t('wl.conditions.subject.variable'));
    default:
      return { text: subject.label, fallback: true };
  }
}

export function parameterLabel(t: TFunction, kind: ParameterKind): string {
  switch (kind) {
    case 'header_name':
      return t('wl.conditions.param.headerName');
    case 'json_path':
      return t('wl.conditions.param.jsonPath');
    case 'name':
      return t('wl.conditions.param.name');
    case 'variable':
      return t('wl.conditions.param.variable');
  }
}

export function parameterPlaceholder(t: TFunction, kind: ParameterKind): string {
  switch (kind) {
    case 'header_name':
      return t('wl.conditions.placeholder.headerName');
    case 'json_path':
      return t('wl.conditions.placeholder.jsonPath');
    case 'name':
      return t('wl.conditions.placeholder.name');
    case 'variable':
      return t('wl.conditions.placeholder.variable');
  }
}

export function issueText(t: TFunction, issue: RowIssue): string {
  if (issue.message !== undefined) {
    return issue.message;
  }
  const detail = issue.detail ?? '';
  switch (issue.code) {
    case 'subject_required':
      return t('wl.conditions.issue.subjectRequired');
    case 'subject_unknown':
      return t('wl.conditions.issue.subjectUnknown', { detail });
    case 'subject_trigger':
      return t('wl.conditions.issue.subjectTrigger');
    case 'parameter_required':
      return t('wl.conditions.issue.parameterRequired');
    case 'parameter_invalid':
      return t('wl.conditions.issue.parameterInvalid');
    case 'operator_required':
      return t('wl.conditions.issue.operatorRequired');
    case 'operator_unknown':
      return t('wl.conditions.issue.operatorUnknown', { detail });
    case 'operator_not_allowed':
      return t('wl.conditions.issue.operatorNotAllowed');
    case 'arg_required':
      return t('wl.conditions.issue.argRequired');
    case 'arg_number':
      return t('wl.conditions.issue.argNumber');
    case 'arg_cidr':
      return t('wl.conditions.issue.argCidr', { detail });
    default:
      return t('wl.conditions.issue.invalid');
  }
}
