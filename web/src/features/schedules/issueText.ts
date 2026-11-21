import { useTranslation } from 'react-i18next';
import { ApiError } from '../../api/client';
import { NAME_MAX_LENGTH, type DraftIssue } from './scheduleDraft';

export interface ScheduleTextLimits {
  minimumIntervalSeconds: number;
  maxSchedules: number;
}

export type IssueText = (issue: DraftIssue) => string;
export type ErrorText = (error: unknown) => string;

const SECONDS_PER_MINUTE = 60;

export function useScheduleIssueText(limits: ScheduleTextLimits): IssueText {
  const { t } = useTranslation();
  const minutes = Math.ceil(limits.minimumIntervalSeconds / SECONDS_PER_MINUTE);
  return (issue) => {
    switch (issue.code) {
      case 'required':
        return t('wl.schedules.error.required');
      case 'too_long':
        return t('wl.schedules.error.tooLong', { max: NAME_MAX_LENGTH });
      case 'cron_invalid':
        return t('wl.schedules.error.cronInvalid');
      case 'time_invalid':
        return t('wl.schedules.error.timeInvalid');
      case 'every_invalid':
        return t('wl.schedules.error.everyInvalid');
      case 'interval_too_short':
        return t('wl.schedules.error.intervalTooShort', { minutes });
      case 'timezone_invalid':
        return t('wl.schedules.error.timezoneInvalid');
      case 'template_invalid':
        return t('wl.schedules.error.templateInvalid');
      case 'secret_unknown':
        return t('wl.schedules.error.secretUnknown');
      case 'secret_not_allowed':
        return t('wl.schedules.error.secretNotAllowed');
      default:
        return issue.message === '' ? t('wl.schedules.error.invalid') : issue.message;
    }
  };
}

export function useScheduleErrorText(limits: ScheduleTextLimits): ErrorText {
  const { t } = useTranslation();
  return (error) => {
    const code = error instanceof ApiError ? error.code : null;
    switch (code) {
      case 'owner_required':
        return t('wl.schedules.error.ownerRequired');
      case 'limit_reached':
        return t('wl.schedules.error.limitReached', { max: limits.maxSchedules });
      case 'schedule_disabled':
        return t('wl.schedules.error.scheduleDisabled');
      default:
        return t('wl.schedules.error.generic');
    }
  };
}
