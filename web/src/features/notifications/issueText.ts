import { useCallback } from 'react';
import { useTranslation } from 'react-i18next';
import { ApiError } from '../../api/client';
import type { IssueCode } from './channelDraft';

export function useIssueText(): (code: IssueCode) => string {
  const { t } = useTranslation();
  return useCallback(
    (code: IssueCode): string => {
      switch (code) {
        case 'required':
          return t('wl.notifications.issue.required');
        case 'nameTooLong':
          return t('wl.notifications.issue.nameTooLong');
        case 'perHour':
          return t('wl.notifications.issue.perHour');
        case 'targetRefused':
          return t('wl.notifications.issue.targetRefused');
        case 'hostNotAllowed':
          return t('wl.notifications.issue.hostNotAllowed');
        case 'invalidFormat':
          return t('wl.notifications.issue.invalidFormat');
        case 'notVerified':
          return t('wl.notifications.issue.notVerified');
        case 'unknown':
          return t('wl.notifications.issue.unknown');
      }
    },
    [t],
  );
}

export function useErrorText(): (error: unknown) => string {
  const { t } = useTranslation();
  return useCallback(
    (error: unknown): string => {
      if (!(error instanceof ApiError)) {
        return t('wl.notifications.error.generic');
      }
      if (error.code === 'feature_disabled') {
        return t('wl.notifications.unavailable');
      }
      if (error.code === 'creator_only') {
        return t('wl.notifications.error.creatorOnly');
      }
      if (error.code === 'limit_reached') {
        return t('wl.notifications.error.limit', { max: error.limit?.max ?? 0 });
      }
      if (error.status === 404) {
        return t('wl.notifications.error.notFound');
      }
      if (error.status === 429) {
        return error.retryAfter === null
          ? t('wl.notifications.error.rateLimitedNoWait')
          : t('wl.notifications.error.rateLimited', { seconds: error.retryAfter });
      }
      return t('wl.notifications.error.generic');
    },
    [t],
  );
}
