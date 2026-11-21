import { useEffect, useId, useMemo, useRef, useState } from 'react';
import { useTranslation } from 'react-i18next';
import { ApiError } from '../../api/client';
import {
  useCreateSchedule,
  useDeleteSchedule,
  useRunSchedule,
  useSchedules,
  useUpdateSchedule,
} from '../../api/queries';
import type { Schedule } from '../../api/types';
import { ConfirmDialog } from '../../components/ConfirmDialog/ConfirmDialog';
import { ConflictBanner } from '../../components/ConflictBanner/ConflictBanner';
import { useNow } from '../endpoints/useNow';
import { DraftFooter } from '../workspace/DraftFooter';
import { useSettingsDraft } from '../workspace/useSettingsDraft';
import { cadenceToExpression } from './cadence';
import { CadenceField } from './CadenceField';
import { nextRuns } from './cron';
import { useScheduleErrorText, useScheduleIssueText, type ScheduleTextLimits } from './issueText';
import { RequestFields, type RequestPatch } from './RequestFields';
import { ScheduleHistory } from './ScheduleHistory';
import {
  blankScheduleDraft,
  firstIssue,
  issuesFromServer,
  NO_ISSUES,
  scheduleDraftOf,
  scheduleInputOf,
  serializeScheduleDraft,
  validateScheduleDraft,
  type IssueMap,
  type ScheduleDraft,
} from './scheduleDraft';
import styles from './ScheduleEditor.module.css';

export interface ScheduleEditorProps {
  endpointId: string;
  schedule: Schedule | null;
  readOnly: boolean;
  limits: ScheduleTextLimits;
  onCreated: (scheduleId: string) => void;
  onDeleted: () => void;
}

const PREVIEW_COUNT = 5;

function keepDraft(draft: ScheduleDraft): ScheduleDraft {
  return draft;
}

function focusFirstInvalid(form: HTMLElement | null): void {
  form?.querySelector<HTMLElement>('[aria-invalid="true"]')?.focus();
}

export function ScheduleEditor({ endpointId, schedule, readOnly, limits, onCreated, onDeleted }: ScheduleEditorProps) {
  const { t } = useTranslation();
  const baseId = useId();
  const formRef = useRef<HTMLFieldSetElement>(null);
  const issueText = useScheduleIssueText(limits);
  const errorText = useScheduleErrorText(limits);
  const now = useNow();
  const listQuery = useSchedules(endpointId, true);
  const create = useCreateSchedule(endpointId);
  const update = useUpdateSchedule(endpointId);
  const pause = useUpdateSchedule(endpointId);
  const run = useRunSchedule(endpointId);
  const remove = useDeleteSchedule(endpointId);
  const [confirmDelete, setConfirmDelete] = useState(false);
  const [queued, setQueued] = useState(false);
  const scheduleId = schedule === null ? null : schedule.id;
  const source = useMemo(() => (schedule === null ? blankScheduleDraft() : scheduleDraftOf(schedule)), [schedule]);

  async function writeDraft(draft: ScheduleDraft): Promise<ScheduleDraft> {
    const input = scheduleInputOf(draft);
    if (scheduleId === null) {
      const created = await create.mutateAsync(input);
      onCreated(created.id);
      return scheduleDraftOf(created);
    }
    const saved = await update.mutateAsync({ scheduleId, input });
    return scheduleDraftOf(saved);
  }

  const settings = useSettingsDraft<ScheduleDraft, ScheduleDraft>({
    source,
    toDraft: keepDraft,
    serialize: serializeScheduleDraft,
    refresh: async () => {
      if (scheduleId === null) {
        return undefined;
      }
      const result = await listQuery.refetch();
      const found = result.data?.items.find((item) => item.id === scheduleId);
      return found === undefined ? undefined : scheduleDraftOf(found);
    },
    write: writeDraft,
    validate: (draft) => validateScheduleDraft(draft, limits.minimumIntervalSeconds).size === 0,
    readOnly,
  });
  const { attempts } = settings;
  const draft = settings.draft;
  const preview = useMemo(
    () => (draft === null ? null : nextRuns(cadenceToExpression(draft.cadence), draft.timezone, now, PREVIEW_COUNT)),
    [draft, now],
  );

  useEffect(() => {
    if (attempts > 0) {
      focusFirstInvalid(formRef.current);
    }
  }, [attempts]);

  if (draft === null) {
    return (
      <p role="status" className={styles.muted}>
        {t('wl.workspace.loading')}
      </p>
    );
  }

  const clientIssues = validateScheduleDraft(draft, limits.minimumIntervalSeconds);
  const serverIssues: IssueMap =
    settings.error instanceof ApiError && settings.error.details.length > 0
      ? issuesFromServer(settings.error.details)
      : NO_ISSUES;
  const issues: IssueMap = attempts === 0 ? NO_ISSUES : clientIssues.size > 0 ? clientIssues : serverIssues;
  const nameIssue = issues.get('name');
  const cadenceIssue = firstIssue(issues, ['every', 'time', 'cron']);
  const timezoneIssue = issues.get('timezone') ?? null;
  const hasFieldErrors = settings.error instanceof ApiError && settings.error.details.length > 0;
  const saveFailure = settings.error === null || hasFieldErrors ? null : errorText(settings.error);
  const nameId = `${baseId}-name`;
  const nameErrorId = `${nameId}-error`;
  const pauseLabelId = `${baseId}-pause`;

  function togglePause(): void {
    if (schedule === null) {
      return;
    }
    setQueued(false);
    pause.mutate({ scheduleId: schedule.id, input: { enabled: !schedule.enabled } });
  }

  function runNow(): void {
    if (schedule === null) {
      return;
    }
    run.mutate(schedule.id, {
      onSuccess: () => {
        setQueued(true);
      },
    });
  }

  function deleteSchedule(): void {
    if (schedule === null) {
      return;
    }
    remove.mutate(schedule.id, {
      onSuccess: () => {
        onDeleted();
      },
    });
  }

  return (
    <div className={styles.page}>
      <div className={styles.header}>
        <h3 className={styles.title}>{schedule === null ? t('wl.schedules.new') : schedule.name}</h3>
        {schedule === null ? null : (
          <span className={schedule.enabled ? styles.active : styles.paused}>
            {schedule.enabled ? t('wl.schedules.status.active') : t('wl.schedules.status.paused')}
          </span>
        )}
      </div>
      {schedule === null || schedule.paused_reason === null ? null : (
        <p className={styles.reason}>{t('wl.schedules.pausedReason', { reason: schedule.paused_reason })}</p>
      )}
      {settings.conflict ? <ConflictBanner onReload={settings.reload} onOverwrite={settings.overwrite} /> : null}
      {saveFailure === null ? null : (
        <p role="alert" className={styles.error}>
          {saveFailure}
        </p>
      )}
      {schedule === null ? null : (
        <div className={styles.toolbar}>
          <div className={styles.switchRow}>
            <span id={pauseLabelId} className={styles.label}>
              {t('wl.schedules.pause.label')}
            </span>
            <button
              type="button"
              role="switch"
              aria-checked={schedule.enabled}
              aria-labelledby={pauseLabelId}
              className={schedule.enabled ? `${styles.switch} ${styles.on}` : styles.switch}
              disabled={readOnly || pause.isPending}
              onClick={togglePause}
            >
              <span className={styles.knob} aria-hidden="true" />
              <span>{schedule.enabled ? t('wl.schedules.pause.on') : t('wl.schedules.pause.off')}</span>
            </button>
          </div>
          <button type="button" className={styles.secondary} disabled={readOnly || run.isPending} onClick={runNow}>
            {t('wl.schedules.action.run')}
          </button>
          <button
            type="button"
            className={styles.danger}
            disabled={readOnly}
            onClick={() => {
              setConfirmDelete(true);
            }}
          >
            {t('wl.schedules.action.delete')}
          </button>
        </div>
      )}
      {pause.isError ? (
        <p role="alert" className={styles.error}>
          {errorText(pause.error)}
        </p>
      ) : null}
      {run.isError ? (
        <p role="alert" className={styles.error}>
          {errorText(run.error)}
        </p>
      ) : null}
      {queued ? (
        <p role="status" className={styles.muted}>
          {t('wl.schedules.action.runQueued')}
        </p>
      ) : null}
      <fieldset ref={formRef} className={styles.form} disabled={readOnly} aria-busy={settings.saving}>
        <div className={styles.field}>
          <label htmlFor={nameId} className={styles.label}>
            {t('wl.schedules.editor.nameLabel')}
          </label>
          <input
            id={nameId}
            type="text"
            className={styles.input}
            aria-invalid={nameIssue === undefined ? undefined : true}
            aria-describedby={nameIssue === undefined ? undefined : nameErrorId}
            value={draft.name}
            onChange={(event) => {
              settings.setDraft({ ...draft, name: event.target.value });
            }}
          />
          {nameIssue === undefined ? null : (
            <p id={nameErrorId} className={styles.error}>
              {issueText(nameIssue)}
            </p>
          )}
        </div>
        <RequestFields
          draft={draft}
          issues={issues}
          issueText={issueText}
          readOnly={readOnly}
          onChange={(patch: RequestPatch) => {
            settings.setDraft({ ...draft, ...patch });
          }}
        />
        <CadenceField
          value={draft.cadence}
          timezone={draft.timezone}
          nextRuns={preview}
          issue={cadenceIssue}
          timezoneIssue={timezoneIssue}
          limits={limits}
          readOnly={readOnly}
          onChange={(cadence) => {
            settings.setDraft({ ...draft, cadence });
          }}
          onTimezoneChange={(timezone) => {
            settings.setDraft({ ...draft, timezone });
          }}
        />
      </fieldset>
      <DraftFooter status={settings} readOnly={readOnly} />
      {schedule === null ? null : <ScheduleHistory endpointId={endpointId} scheduleId={schedule.id} />}
      <ConfirmDialog
        open={confirmDelete}
        title={t('wl.schedules.delete.title')}
        body={t('wl.schedules.delete.body')}
        confirmLabel={t('wl.schedules.delete.confirm')}
        cancelLabel={t('wl.schedules.delete.cancel')}
        busy={remove.isPending}
        onConfirm={deleteSchedule}
        onOpenChange={setConfirmDelete}
      />
    </div>
  );
}
