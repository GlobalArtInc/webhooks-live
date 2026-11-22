import * as Dialog from '@radix-ui/react-dialog';
import { useId, useState, type FormEvent } from 'react';
import { useTranslation } from 'react-i18next';
import { ApiError } from '../../api/client';
import { useCapture, useEndpoint, useResendCapture } from '../../api/queries';
import type { ForwardJob } from '../../api/types';
import { ForwardResultPanel } from '../../components/ForwardResultPanel/ForwardResultPanel';
import { useReturnFocus } from '../../components/useReturnFocus';
import { issueFromServer, validateTarget, type RuleIssue, TARGET_MAX_CHARS } from './ruleDraft';
import { ModalContent } from '../../components/ModalContent/ModalContent';
import styles from './ResendDialog.module.css';

export interface ResendDialogProps {
  endpointId: string;
  captureId: string;
  open: boolean;
  onOpenChange: (open: boolean) => void;
}

interface ResendFormProps {
  endpointId: string;
  captureId: string;
}

const CUSTOM_TARGET = '';

function ResendForm({ endpointId, captureId }: ResendFormProps) {
  const { t } = useTranslation();
  const baseId = useId();
  const endpoint = useEndpoint(endpointId).data;
  const detail = useCapture(endpointId, captureId);
  const resend = useResendCapture(endpointId);
  const [targetUrl, setTargetUrl] = useState('');
  const [method, setMethod] = useState('');
  const [attempted, setAttempted] = useState(false);
  const [job, setJob] = useState<ForwardJob | null>(null);
  const rules = endpoint?.forward_rules ?? [];
  const clientIssue = validateTarget(targetUrl);
  const failure: unknown = resend.error;
  const serverIssue = failure instanceof ApiError && failure.status === 422 ? issueFromServer(failure.details) : null;
  const targetIssue: RuleIssue | null = (attempted ? clientIssue : null) ?? serverIssue;
  const genericFailure = failure !== null && !(failure instanceof ApiError && failure.status === 422);
  const shown = job === null ? null : (detail.data?.forwards.find((item) => item.id === job.id) ?? job);

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

  function send(): void {
    setAttempted(true);
    if (clientIssue !== null) {
      return;
    }
    const trimmedMethod = method.trim();
    resend.mutate(
      {
        captureId,
        input: { target_url: targetUrl.trim(), method: trimmedMethod === '' ? null : trimmedMethod },
      },
      {
        onSuccess: (result) => {
          setJob(result);
        },
      },
    );
  }

  function submit(event: FormEvent<HTMLFormElement>): void {
    event.preventDefault();
    send();
  }

  const targetId = `${baseId}-target`;
  const prefillId = `${baseId}-prefill`;
  const methodId = `${baseId}-method`;
  const errorId = `${targetId}-error`;

  return (
    <form noValidate className={styles.form} aria-busy={resend.isPending} onSubmit={submit}>
      {rules.length === 0 ? null : (
        <div className={styles.field}>
          <label htmlFor={prefillId} className={styles.label}>
            {t('wl.forwarding.resend.prefill')}
          </label>
          <select
            id={prefillId}
            className={styles.input}
            defaultValue={CUSTOM_TARGET}
            onChange={(event) => {
              if (event.target.value !== CUSTOM_TARGET) {
                setTargetUrl(event.target.value);
              }
            }}
          >
            <option value={CUSTOM_TARGET}>{t('wl.forwarding.resend.custom')}</option>
            {rules.map((rule) => (
              <option key={rule.id} value={rule.target_url}>
                {rule.target_url}
              </option>
            ))}
          </select>
        </div>
      )}
      <div className={styles.field}>
        <label htmlFor={targetId} className={styles.label}>
          {t('wl.forwarding.resend.target')}
        </label>
        <input
          id={targetId}
          type="text"
          inputMode="url"
          spellCheck={false}
          autoComplete="off"
          className={styles.input}
          aria-invalid={targetIssue === null ? undefined : true}
          aria-describedby={targetIssue === null ? undefined : errorId}
          value={targetUrl}
          onChange={(event) => {
            setTargetUrl(event.target.value);
          }}
        />
        {targetIssue === null ? null : (
          <p id={errorId} className={styles.error}>
            {issueMessage(targetIssue)}
          </p>
        )}
      </div>
      <div className={styles.field}>
        <label htmlFor={methodId} className={styles.label}>
          {t('wl.forwarding.resend.method')}
        </label>
        <input
          id={methodId}
          type="text"
          spellCheck={false}
          autoComplete="off"
          className={styles.input}
          value={method}
          onChange={(event) => {
            setMethod(event.target.value);
          }}
        />
        <p className={styles.hint}>{t('wl.forwarding.rule.methodHint')}</p>
      </div>
      {genericFailure ? (
        <p role="alert" className={styles.error}>
          {t('wl.forwarding.resend.failed')}
        </p>
      ) : null}
      {shown === null ? null : <ForwardResultPanel results={[shown]} onResend={send} />}
      <div className={styles.actions}>
        <button type="submit" className={styles.primary} disabled={resend.isPending}>
          {resend.isPending ? t('wl.forwarding.resend.sending') : t('wl.forwarding.resend.send')}
        </button>
      </div>
    </form>
  );
}

export function ResendDialog({ endpointId, captureId, open, onOpenChange }: ResendDialogProps) {
  const { t } = useTranslation();
  const returnFocus = useReturnFocus();
  return (
    <Dialog.Root open={open} onOpenChange={onOpenChange}>
      <Dialog.Portal>
        <Dialog.Overlay className={styles.overlay} />
        <ModalContent
          className={styles.content}
          onOpenAutoFocus={returnFocus.onOpenAutoFocus}
          onCloseAutoFocus={returnFocus.onCloseAutoFocus}
        >
          <Dialog.Title className={styles.title}>{t('wl.forwarding.resend.title')}</Dialog.Title>
          <Dialog.Description className={styles.body}>{t('wl.forwarding.resend.body')}</Dialog.Description>
          <ResendForm endpointId={endpointId} captureId={captureId} />
          <div className={styles.close}>
            <Dialog.Close asChild>
              <button type="button" className={styles.secondary}>
                {t('wl.forwarding.resend.close')}
              </button>
            </Dialog.Close>
          </div>
        </ModalContent>
      </Dialog.Portal>
    </Dialog.Root>
  );
}
