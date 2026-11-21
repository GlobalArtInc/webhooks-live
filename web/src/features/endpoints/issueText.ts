import { useCallback } from 'react';
import { useTranslation } from 'react-i18next';
import {
  DELAY_MAX_MS,
  FIELD_PATHS,
  MAX_RESPONSE_HEADERS,
  STATUS_MAX,
  STATUS_MIN,
  type FieldIssue,
} from './responseDraft';

export type IssueText = (path: string, issue: FieldIssue) => string;

export function useIssueText(): IssueText {
  const { t } = useTranslation();

  return useCallback<IssueText>(
    (path, issue) => {
      switch (issue.code) {
        case 'required':
          return t('wl.home.error.required');
        case 'invalid':
          return t('wl.home.error.invalid');
        case 'tooLong':
          return t('wl.home.error.tooLong', { limit: issue.limit ?? 0 });
        case 'outOfRange':
          return path === FIELD_PATHS.delay
            ? t('wl.home.error.delayRange', { max: DELAY_MAX_MS })
            : t('wl.home.error.statusRange', { min: STATUS_MIN, max: STATUS_MAX });
        case 'duplicate':
          return t('wl.home.error.duplicate');
        case 'notAllowed':
          return t('wl.home.error.notAllowed');
        case 'tooMany':
          return t('wl.home.error.tooMany', { limit: issue.limit ?? MAX_RESPONSE_HEADERS });
      }
    },
    [t],
  );
}
