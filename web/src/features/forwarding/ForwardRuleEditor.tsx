import { useEffect, useId, useRef, useState, type FormEvent, type ReactNode } from 'react';
import { useTranslation } from 'react-i18next';
import { ApiError } from '../../api/client';
import { useCreateForwardRule, usePatchForwardRule } from '../../api/forwardRules';
import type { ForwardRule } from '../../api/types';
import {
  issueFromServer,
  draftFromRule,
  ruleInputFromDraft,
  validateTarget,
  type RuleDraft,
  type RuleIssue,
  TARGET_MAX_CHARS,
} from './ruleDraft';
import styles from './ForwardRuleEditor.module.css';

const LIMIT_CODE = 'forward_rule_limit';

export interface ForwardRuleEditorProps {
  endpointId: string;
  rule: ForwardRule | null;
  onDone: () => void;
  onCancel: () => void;
}

interface FieldProps {
  id: string;
  label: string;
  hint?: string;
  message: string | null;
  children: ReactNode;
}

function Field({ id, label, hint, message, children }: FieldProps) {
  return (
    <div className={styles.field}>
      <label htmlFor={id} className={styles.label}>
        {label}
      </label>
      {children}
      {hint === undefined ? null : <p className={styles.hint}>{hint}</p>}
      {message === null ? null : (
        <p id={`${id}-error`} className={styles.error}>
          {message}
        </p>
      )}
    </div>
  );
}

export function ForwardRuleEditor({ endpointId, rule, onDone, onCancel }: ForwardRuleEditorProps) {
  const { t } = useTranslation();
  const baseId = useId();
  const targetRef = useRef<HTMLInputElement>(null);
  const create = useCreateForwardRule(endpointId);
  const patch = usePatchForwardRule(endpointId);
  const [draft, setDraft] = useState<RuleDraft>(() => draftFromRule(rule));
  const [attempted, setAttempted] = useState(false);
  const busy = create.isPending || patch.isPending;
  const failure: unknown = create.error ?? patch.error;
  const serverIssue = failure instanceof ApiError && failure.status === 422 ? issueFromServer(failure.details) : null;
  const clientIssue = validateTarget(draft.targetUrl);
  const targetIssue = (attempted ? clientIssue : null) ?? serverIssue;
  const limitReached = failure instanceof ApiError && failure.code === LIMIT_CODE;
  const genericFailure = failure !== null && !(failure instanceof ApiError && failure.status === 422) && !limitReached;

  function issueMessage(issue: RuleIssue): string {
    switch (issue.code) {
      case 'required':
        return t('wl.forwarding.error.required');
      case 'invalid':
        return t('wl.forwarding.error.invalid');
      case 'tooLong':
        return t('wl.forwarding.error.tooLong', { limit: TARGET_MAX_CHARS });
      case 'targetRefused':
        return t('wl.forwarding.error.targetRefused', { reason: issue.detail ?? '' });
      case 'targetUnresolvable':
        return t('wl.forwarding.error.targetUnresolvable');
    }
  }

  useEffect(() => {
    if (serverIssue !== null) {
      targetRef.current?.focus();
    }
  }, [serverIssue]);

  function submit(event: FormEvent<HTMLFormElement>): void {
    event.preventDefault();
    setAttempted(true);
    if (clientIssue !== null) {
      targetRef.current?.focus();
      return;
    }
    const enabled = rule?.enabled ?? true;
    const input = ruleInputFromDraft(draft, enabled);
    if (rule === null) {
      create.mutate(input, { onSuccess: onDone });
    } else {
      patch.mutate({ ruleId: rule.id, input }, { onSuccess: onDone });
    }
  }

  const targetId = `${baseId}-target`;
  const methodId = `${baseId}-method`;
  const contentTypeId = `${baseId}-content-type`;
  const headersId = `${baseId}-headers`;
  const appendId = `${baseId}-append`;

  return (
    <form noValidate className={styles.form} aria-busy={busy} onSubmit={submit}>
      <Field
        id={targetId}
        label={t('wl.forwarding.rule.target')}
        message={targetIssue === null ? null : issueMessage(targetIssue)}
      >
        <input
          ref={targetRef}
          id={targetId}
          type="text"
          inputMode="url"
          spellCheck={false}
          autoComplete="off"
          className={styles.input}
          aria-invalid={targetIssue === null ? undefined : true}
          aria-describedby={targetIssue === null ? undefined : `${targetId}-error`}
          value={draft.targetUrl}
          onChange={(event) => {
            setDraft({ ...draft, targetUrl: event.target.value });
          }}
        />
      </Field>
      <div className={styles.grid}>
        <Field
          id={methodId}
          label={t('wl.forwarding.rule.method')}
          hint={t('wl.forwarding.rule.methodHint')}
          message={null}
        >
          <input
            id={methodId}
            type="text"
            spellCheck={false}
            autoComplete="off"
            className={styles.input}
            value={draft.method}
            onChange={(event) => {
              setDraft({ ...draft, method: event.target.value });
            }}
          />
        </Field>
        <Field
          id={contentTypeId}
          label={t('wl.forwarding.rule.contentType')}
          hint={t('wl.forwarding.rule.contentTypeHint')}
          message={null}
        >
          <input
            id={contentTypeId}
            type="text"
            spellCheck={false}
            autoComplete="off"
            className={styles.input}
            value={draft.contentType}
            onChange={(event) => {
              setDraft({ ...draft, contentType: event.target.value });
            }}
          />
        </Field>
      </div>
      <Field
        id={headersId}
        label={t('wl.forwarding.rule.headers')}
        hint={t('wl.forwarding.rule.headersHint')}
        message={null}
      >
        <input
          id={headersId}
          type="text"
          spellCheck={false}
          autoComplete="off"
          className={styles.input}
          value={draft.headers}
          onChange={(event) => {
            setDraft({ ...draft, headers: event.target.value });
          }}
        />
      </Field>
      <div className={styles.checkRow}>
        <input
          id={appendId}
          type="checkbox"
          checked={draft.appendPath}
          onChange={(event) => {
            setDraft({ ...draft, appendPath: event.target.checked });
          }}
        />
        <label htmlFor={appendId}>{t('wl.forwarding.rule.appendPath')}</label>
      </div>
      {limitReached ? (
        <p role="alert" className={styles.error}>
          {t('wl.forwarding.error.limit')}
        </p>
      ) : null}
      {genericFailure ? (
        <p role="alert" className={styles.error}>
          {t('wl.forwarding.error.save')}
        </p>
      ) : null}
      <div className={styles.actions}>
        <button type="button" className={styles.secondary} onClick={onCancel}>
          {t('wl.forwarding.rule.cancel')}
        </button>
        <button type="submit" className={styles.primary} disabled={busy}>
          {t('wl.forwarding.rule.save')}
        </button>
      </div>
    </form>
  );
}
