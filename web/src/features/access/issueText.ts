import { useTranslation } from 'react-i18next';
import { PASSWORD_MAX, PASSWORD_MIN, SECRET_MAX_BYTES } from './secrets';
import type { AccessIssue } from './types';

export type IssueText = (issue: AccessIssue) => string;

export function useAccessIssueText(): IssueText {
  const { t } = useTranslation();
  return (issue) => {
    switch (issue.reason) {
      case 'passwordShort':
        return t('wl.access.issue.passwordShort', { min: PASSWORD_MIN });
      case 'passwordLong':
        return t('wl.access.issue.passwordLong', { max: PASSWORD_MAX });
      case 'passwordMismatch':
        return t('wl.access.issue.passwordMismatch');
      case 'cidrsEmpty':
        return t('wl.access.issue.cidrsEmpty');
      case 'authNameRequired':
        return t('wl.access.issue.authNameRequired');
      case 'authNameInvalid':
        return t('wl.access.issue.authNameInvalid', { max: SECRET_MAX_BYTES });
      case 'authSecretRequired':
        return t('wl.access.issue.authSecretRequired');
      case 'authSecretInvalid':
        return t('wl.access.issue.authSecretInvalid', { max: SECRET_MAX_BYTES });
      case 'notAllowed':
        return t('wl.access.issue.notAllowed');
      case 'unknownOutcome':
        return t('wl.access.issue.unknownOutcome');
      case 'rejected':
        return t('wl.access.issue.rejected', { reason: issue.detail ?? '' });
    }
  };
}
