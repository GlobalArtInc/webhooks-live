import { useCallback, useEffect, useId, useRef, type ReactNode } from 'react';
import { useTranslation } from 'react-i18next';
import { ApiError } from '../../../api/client';
import { useEndpoint, useInstance, useUpdateEndpoint } from '../../../api/queries';
import type { Endpoint } from '../../../api/types';
import { ConflictBanner } from '../../../components/ConflictBanner/ConflictBanner';
import { useViewportLayout } from '../../endpoint/useViewport';
import { useIssueText } from '../../endpoints/issueText';
import { TemplateEditor } from '../../templating/TemplateEditor';
import { TemplateWorkbench } from '../../templating/TemplateWorkbench';
import { useTemplating } from '../../templating/useTemplating';
import { VariablesSwitch } from '../../templating/VariablesSwitch';
import {
  BODY_MAX_BYTES,
  DELAY_MAX_MS,
  FIELD_PATHS,
  issuesFromServer,
  NO_ISSUES,
  STATUS_MAX,
  STATUS_MIN,
  validateDraft,
  type IssueMap,
} from '../../endpoints/responseDraft';
import type { SectionProps } from '../sections';
import { DraftFooter } from '../DraftFooter';
import { useSettingsDraft } from '../useSettingsDraft';
import {
  responseSettingsDraftOf,
  responseSettingsInput,
  serializeResponseSettings,
  type ResponseSettingsDraft,
} from './responseSettings';
import styles from './ResponseSection.module.css';

interface FieldProps {
  id: string;
  label: string;
  message: string | null;
  children: ReactNode;
}

function Field({ id, label, message, children }: FieldProps) {
  return (
    <div className={styles.field}>
      <label htmlFor={id} className={styles.label}>
        {label}
      </label>
      {children}
      {message !== null && (
        <p id={`${id}-error`} className={styles.error}>
          {message}
        </p>
      )}
    </div>
  );
}

function focusFirstInvalid(form: HTMLElement | null): void {
  form?.querySelector<HTMLElement>('[aria-invalid="true"]')?.focus();
}

export function ResponseSection({ endpointId }: SectionProps) {
  const { t } = useTranslation();
  const issueText = useIssueText();
  const baseId = useId();
  const statusId = `${baseId}-status`;
  const contentTypeId = `${baseId}-content-type`;
  const delayId = `${baseId}-delay`;
  const corsId = `${baseId}-cors`;
  const ringName = `${baseId}-ring`;
  const formRef = useRef<HTMLFieldSetElement>(null);
  const blockedRef = useRef(false);
  const layout = useViewportLayout();
  const endpointQuery = useEndpoint(endpointId);
  const endpoint: Endpoint | undefined = endpointQuery.data;
  const instance = useInstance();
  const update = useUpdateEndpoint(endpointId);
  const readOnly = endpoint?.my_role === 'viewer';
  const settings = useSettingsDraft<ResponseSettingsDraft, Endpoint>({
    source: endpoint,
    toDraft: responseSettingsDraftOf,
    serialize: serializeResponseSettings,
    refresh: async () => (await endpointQuery.refetch()).data,
    write: (draft) => update.mutateAsync(responseSettingsInput(draft)),
    validate: (draft) => validateDraft(draft).size === 0 && !blockedRef.current,
    readOnly,
  });
  const { attempts } = settings;
  const draft = settings.draft;
  const ringAllowed = instance.data?.features.ring_mode_allowed === true;
  const maxBodyBytes = instance.data?.limits.max_response_body_bytes ?? BODY_MAX_BYTES;

  const focusField = useCallback(
    (field: string) => {
      const target = field === 'status' ? statusId : field === 'content_type' ? contentTypeId : null;
      if (target !== null) {
        document.getElementById(target)?.focus();
      }
    },
    [statusId, contentTypeId],
  );
  const templating = useTemplating({
    endpointId,
    endpointUrl: endpoint?.url ?? '',
    draft,
    maxBodyBytes,
    focusField,
  });
  const { blocked } = templating;
  const refreshPreview = templating.preview.refresh;

  useEffect(() => {
    blockedRef.current = blocked;
  }, [blocked]);

  useEffect(() => {
    if (attempts > 0) {
      focusFirstInvalid(formRef.current);
    }
  }, [attempts]);

  if (endpoint === undefined || draft === null) {
    return (
      <p role="status" className={styles.loading}>
        {t('wl.workspace.loading')}
      </p>
    );
  }

  const clientIssues: IssueMap = validateDraft(draft);
  const serverIssues: IssueMap =
    settings.error instanceof ApiError && settings.error.status === 422
      ? issuesFromServer(settings.error.details)
      : NO_ISSUES;
  const issues: IssueMap = attempts === 0 ? NO_ISSUES : clientIssues.size > 0 ? clientIssues : serverIssues;

  function messageFor(path: string): string | null {
    const issue = issues.get(path);
    return issue === undefined ? null : issueText(path, issue);
  }

  function describedBy(id: string, path: string): string | undefined {
    return issues.has(path) ? `${id}-error` : undefined;
  }

  const form = (
    <fieldset ref={formRef} className={styles.form} disabled={readOnly} aria-busy={settings.saving}>
      <VariablesSwitch
        checked={draft.templated}
        disabled={readOnly}
        onChange={(templated) => {
          settings.setDraft({ ...draft, templated });
        }}
      />
      <div className={styles.grid}>
        <Field id={statusId} label={t('wl.home.field.status.label')} message={messageFor(FIELD_PATHS.status)}>
          <input
            id={statusId}
            type="number"
            inputMode="numeric"
            min={STATUS_MIN}
            max={STATUS_MAX}
            step={1}
            className={styles.input}
            aria-invalid={issues.has(FIELD_PATHS.status) ? true : undefined}
            aria-describedby={describedBy(statusId, FIELD_PATHS.status)}
            value={draft.status}
            onChange={(event) => {
              settings.setDraft({ ...draft, status: event.target.value });
            }}
          />
        </Field>
        <Field
          id={contentTypeId}
          label={t('wl.home.field.contentType.label')}
          message={messageFor(FIELD_PATHS.contentType)}
        >
          <input
            id={contentTypeId}
            type="text"
            className={styles.input}
            aria-invalid={issues.has(FIELD_PATHS.contentType) ? true : undefined}
            aria-describedby={describedBy(contentTypeId, FIELD_PATHS.contentType)}
            value={draft.contentType}
            onChange={(event) => {
              settings.setDraft({ ...draft, contentType: event.target.value });
            }}
          />
        </Field>
        <Field id={delayId} label={t('wl.home.field.delay.label')} message={messageFor(FIELD_PATHS.delay)}>
          <input
            id={delayId}
            type="number"
            inputMode="numeric"
            min={0}
            max={DELAY_MAX_MS}
            step={1}
            className={styles.input}
            aria-invalid={issues.has(FIELD_PATHS.delay) ? true : undefined}
            aria-describedby={describedBy(delayId, FIELD_PATHS.delay)}
            value={draft.delay}
            onChange={(event) => {
              settings.setDraft({ ...draft, delay: event.target.value });
            }}
          />
        </Field>
      </div>
      <TemplateEditor
        draft={draft}
        onDraftChange={settings.setDraft}
        model={templating}
        issues={issues}
        bodyMessage={messageFor(FIELD_PATHS.body)}
        headersMessage={messageFor(FIELD_PATHS.headers)}
        readOnly={readOnly}
        maxBodyBytes={maxBodyBytes}
        onSubmit={refreshPreview}
      />
      <div className={styles.checkRow}>
        <input
          id={corsId}
          type="checkbox"
          checked={draft.cors}
          onChange={(event) => {
            settings.setDraft({ ...draft, cors: event.target.checked });
          }}
        />
        <label htmlFor={corsId}>{t('wl.home.field.cors.label')}</label>
      </div>
      {ringAllowed ? (
        <fieldset className={styles.ring}>
          <legend className={styles.label}>{t('wl.workspace.response.ring.legend')}</legend>
          <label className={styles.checkRow}>
            <input
              type="radio"
              name={ringName}
              value="refuse"
              checked={draft.overflow === 'refuse'}
              onChange={() => {
                settings.setDraft({ ...draft, overflow: 'refuse' });
              }}
            />
            {t('wl.workspace.response.ring.refuse')}
          </label>
          <label className={styles.checkRow}>
            <input
              type="radio"
              name={ringName}
              value="ring"
              checked={draft.overflow === 'ring'}
              onChange={() => {
                settings.setDraft({ ...draft, overflow: 'ring' });
              }}
            />
            {t('wl.workspace.response.ring.ring')}
          </label>
        </fieldset>
      ) : null}
    </fieldset>
  );

  return (
    <div className={styles.page}>
      <h2 className={styles.heading}>{t('wl.workspace.section.response')}</h2>
      {settings.conflict ? <ConflictBanner onReload={settings.reload} onOverwrite={settings.overwrite} /> : null}
      {readOnly ? <p className={styles.note}>{t('wl.templating.readOnly')}</p> : null}
      <TemplateWorkbench model={templating} layout={layout} form={form} />
      <DraftFooter
        status={settings}
        readOnly={readOnly}
        saveBlockedText={blocked ? t('wl.templating.problems.saveBlocked') : undefined}
      />
    </div>
  );
}
