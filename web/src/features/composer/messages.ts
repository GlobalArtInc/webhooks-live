import { useTranslation } from 'react-i18next';
import type { ApiLimit } from '../../api/types';
import { TARGET_MAX_CHARS } from '../forwarding/ruleDraft';
import { BODY_MAX_BYTES } from './draft';
import type { HistoryFailure, SendFailure, UrlIssue } from './sendErrors';

const BODY_LIMIT_LABEL = `${BODY_MAX_BYTES / (1024 * 1024)} MiB`;

export function useComposerMessages() {
  const { t } = useTranslation();

  function quotaSentence(limit: ApiLimit | null): string {
    switch (limit?.name) {
      case 'composer_user_hour':
        return t('wl.composer.error.quota.userHour', { max: limit.max });
      case 'composer_network_day':
        return t('wl.composer.error.quota.networkDay', { max: limit.max });
      case 'composer_endpoint_hour':
        return t('wl.composer.error.quota.endpointHour', { max: limit.max });
      default:
        return t('wl.composer.error.quota.other');
    }
  }

  function urlText(issue: UrlIssue): string {
    switch (issue.code) {
      case 'required':
        return t('wl.composer.url.required');
      case 'invalid':
        return t('wl.composer.url.invalid');
      case 'tooLong':
        return t('wl.composer.url.tooLong', { limit: TARGET_MAX_CHARS });
      case 'targetRefused':
        return t('wl.composer.url.targetRefused', { reason: issue.detail ?? '' });
      case 'targetUnresolvable':
        return t('wl.composer.url.targetUnresolvable');
      case 'unreachable':
        return t('wl.composer.url.unreachable', { reason: issue.detail ?? '' });
    }
  }

  function failureText(failure: SendFailure): string {
    switch (failure.kind) {
      case 'url':
        return urlText(failure.issue);
      case 'quota':
        return failure.retryAfter === null
          ? quotaSentence(failure.limit)
          : `${quotaSentence(failure.limit)} ${t('wl.composer.error.retryIn', { count: failure.retryAfter })}`;
      case 'rateLimited':
        return t('wl.composer.error.rateLimited');
      case 'timeout':
        return t('wl.composer.error.timeout');
      case 'bodyTooLarge':
        return t('wl.composer.error.bodyTooLarge', { limit: BODY_LIMIT_LABEL });
      case 'hopLimit':
        return t('wl.composer.error.hopLimit');
      case 'disabled':
        return t('wl.composer.error.disabled');
      case 'cancelled':
        return t('wl.composer.error.cancelled');
      case 'failed':
        return t('wl.composer.error.failed');
    }
  }

  function historyText(failure: HistoryFailure): string {
    switch (failure) {
      case 'signIn':
        return t('wl.composer.history.signIn');
      case 'disabled':
        return t('wl.composer.history.disabled');
      case 'failed':
        return t('wl.composer.history.failed');
    }
  }

  return { urlText, failureText, historyText };
}
