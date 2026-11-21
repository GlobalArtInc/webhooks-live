import type { TFunction } from 'i18next';
import type { ShapeIssue } from './types';

export function issueMessage(t: TFunction, issue: ShapeIssue): string {
  const limit = issue.limit ?? 0;
  switch (issue.code) {
    case 'tooLong':
      return t('wl.steps.issue.tooLong', { limit });
    case 'tooLarge':
      return t('wl.steps.issue.tooLarge', { limit });
    case 'tooSmall':
      return t('wl.steps.issue.tooSmall', { limit });
    case 'tooBig':
      return t('wl.steps.issue.tooBig', { limit });
    case 'notInteger':
      return t('wl.steps.issue.notInteger');
    case 'badHeaderName':
      return t('wl.steps.issue.badHeaderName');
    case 'badJsonPath':
      return t('wl.steps.issue.badJsonPath');
    case 'badName':
      return t('wl.steps.issue.badName');
    case 'duplicateName':
      return t('wl.steps.issue.duplicateName');
    case 'tooManyRows':
      return t('wl.steps.issue.tooManyRows', { count: limit });
    case 'guardIncomplete':
      return t('wl.steps.issue.guardIncomplete');
  }
}
