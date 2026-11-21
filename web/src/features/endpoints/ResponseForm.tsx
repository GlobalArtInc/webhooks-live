import { useEffect, useId, useMemo, useRef, useState, type FormEvent, type ReactNode } from 'react';
import { useTranslation } from 'react-i18next';
import { ApiError } from '../../api/client';
import { CreateErrorNotice, isValidationFailure } from './CreateErrorNotice';
import { HeadersEditor } from './HeadersEditor';
import { useIssueText } from './issueText';
import { useCreateAndOpen } from './useCreateAndOpen';
import {
  buildEndpointInput,
  createDefaultDraft,
  DELAY_MAX_MS,
  FIELD_PATHS,
  issuesFromServer,
  NO_ISSUES,
  STATUS_MAX,
  STATUS_MIN,
  validateDraft,
  type IssueMap,
  type ResponseDraft,
} from './responseDraft';
import styles from './ResponseForm.module.css';

type DraftPatch = Partial<ResponseDraft>;

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

function focusFirstInvalid(form: HTMLFormElement | null): void {
  form?.querySelector<HTMLElement>('[aria-invalid="true"]')?.focus();
}

export function ResponseForm() {
  const { t } = useTranslation();
  const issueText = useIssueText();
  const { create, isPending, error } = useCreateAndOpen();
  const baseId = useId();
  const formRef = useRef<HTMLFormElement>(null);
  const [draft, setDraft] = useState<ResponseDraft>(createDefaultDraft);
  const [clientIssues, setClientIssues] = useState<IssueMap>(NO_ISSUES);
  const [attempt, setAttempt] = useState(0);

  const serverIssues = useMemo<IssueMap>(
    () => (error instanceof ApiError && error.status === 422 ? issuesFromServer(error.details) : NO_ISSUES),
    [error],
  );
  const issues = clientIssues.size > 0 ? clientIssues : serverIssues;

  useEffect(() => {
    if (attempt > 0) {
      focusFirstInvalid(formRef.current);
    }
  }, [attempt]);

  useEffect(() => {
    if (isValidationFailure(error)) {
      focusFirstInvalid(formRef.current);
    }
  }, [error]);

  function patchDraft(patch: DraftPatch) {
    setDraft((current) => ({ ...current, ...patch }));
  }

  function submitDraft() {
    const found = validateDraft(draft);
    setClientIssues(found);
    setAttempt((count) => count + 1);
    if (found.size === 0) {
      create(buildEndpointInput(draft));
    }
  }

  function handleSubmit(event: FormEvent<HTMLFormElement>) {
    event.preventDefault();
    submitDraft();
  }

  function messageFor(path: string): string | null {
    const issue = issues.get(path);
    return issue === undefined ? null : issueText(path, issue);
  }

  function describedBy(id: string, path: string): string | undefined {
    return issues.has(path) ? `${id}-error` : undefined;
  }

  const statusId = `${baseId}-status`;
  const contentTypeId = `${baseId}-content-type`;
  const bodyId = `${baseId}-body`;
  const delayId = `${baseId}-delay`;
  const corsId = `${baseId}-cors`;

  return (
    <form ref={formRef} noValidate aria-busy={isPending} className={styles.form} onSubmit={handleSubmit}>
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
            onChange={(event) => patchDraft({ status: event.target.value })}
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
            onChange={(event) => patchDraft({ contentType: event.target.value })}
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
            onChange={(event) => patchDraft({ delay: event.target.value })}
          />
        </Field>
      </div>
      <Field id={bodyId} label={t('wl.home.field.body.label')} message={messageFor(FIELD_PATHS.body)}>
        <textarea
          id={bodyId}
          rows={6}
          className={styles.textarea}
          aria-invalid={issues.has(FIELD_PATHS.body) ? true : undefined}
          aria-describedby={describedBy(bodyId, FIELD_PATHS.body)}
          value={draft.body}
          onChange={(event) => patchDraft({ body: event.target.value })}
        />
      </Field>
      <HeadersEditor rows={draft.headers} issues={issues} onChange={(headers) => patchDraft({ headers })} />
      {messageFor(FIELD_PATHS.headers) !== null && <p className={styles.error}>{messageFor(FIELD_PATHS.headers)}</p>}
      <div className={styles.checkRow}>
        <input
          id={corsId}
          type="checkbox"
          checked={draft.cors}
          onChange={(event) => patchDraft({ cors: event.target.checked })}
        />
        <label htmlFor={corsId}>{t('wl.home.field.cors.label')}</label>
      </div>
      {issues.size > 0 && (
        <p role="alert" className={styles.summary}>
          {t('wl.home.error.validation')}
        </p>
      )}
      {error !== null && !isValidationFailure(error) && !isPending && (
        <CreateErrorNotice error={error} onRetry={submitDraft} />
      )}
      <div className={styles.actions}>
        <button type="submit" className={styles.submit} disabled={isPending}>
          {isPending ? t('wl.home.create.busy') : t('wl.home.custom.submit')}
        </button>
      </div>
    </form>
  );
}
