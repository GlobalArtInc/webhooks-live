import type { TFunction } from 'i18next';
import { guardArgumentToText, operatorNeedsArgument } from './model';
import type { GuardOperator, GuardPredicate } from './types';

export function operatorLabel(t: TFunction, op: GuardOperator): string {
  switch (op) {
    case 'eq':
      return t('wl.steps.guard.op.eq');
    case 'ne':
      return t('wl.steps.guard.op.ne');
    case 'contains':
      return t('wl.steps.guard.op.contains');
    case 'starts_with':
      return t('wl.steps.guard.op.startsWith');
    case 'ends_with':
      return t('wl.steps.guard.op.endsWith');
    case 'matches':
      return t('wl.steps.guard.op.matches');
    case 'exists':
      return t('wl.steps.guard.op.exists');
    case 'gt':
      return t('wl.steps.guard.op.gt');
    case 'lt':
      return t('wl.steps.guard.op.lt');
    case 'in':
      return t('wl.steps.guard.op.in');
    case 'in_cidr':
      return t('wl.steps.guard.op.inCidr');
  }
}

export function describeGuard(t: TFunction, guard: GuardPredicate): string {
  const subject = guard.value === '' ? '…' : guard.value;
  const operator = operatorLabel(t, guard.op);
  if (!operatorNeedsArgument(guard.op)) {
    return t('wl.steps.guard.summaryNoArgument', { subject, operator });
  }
  const argument = guardArgumentToText(guard.arg);
  return t('wl.steps.guard.summary', { subject, operator, argument: argument === '' ? '…' : argument });
}
