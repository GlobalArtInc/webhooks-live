import * as Dialog from '@radix-ui/react-dialog';
import { useEffect, useId, useRef, useState, type FormEvent } from 'react';
import { useTranslation } from 'react-i18next';
import { useQueryClient } from '@tanstack/react-query';
import { flattenOwnedEndpoints, tokenQueryKeys, useCreateApiToken, useOwnedEndpoints } from '../../api/tokens';
import { useLanguage } from '../../i18n/provider';
import { useNow } from '../endpoints/useNow';
import {
  draftToInput,
  emptyDraft,
  expiryDate,
  EXPIRY_PRESETS,
  hasProblems,
  pruneEndpointIds,
  toggleEndpoint,
  toggleScope,
  validateDraft,
  writesEndpointsWhileRestricted,
  YEAR_DAYS,
  type TokenDraft,
} from './draft';
import { describeTokenError, type TokenErrorInfo } from './errors';
import { EndpointPicker } from './EndpointPicker';
import { ScopeGrid } from './ScopeGrid';
import { SecretReveal } from './SecretReveal';
import { TokenErrorNotice } from './TokenErrorNotice';
import { formatDate } from './time';
import styles from './CreateTokenDialog.module.css';

export interface CreateTokenDialogProps {
  onClose: () => void;
}

interface RevealedToken {
  secret: string;
  name: string;
}

export function CreateTokenDialog({ onClose }: CreateTokenDialogProps) {
  const { t } = useTranslation();
  const { language } = useLanguage();
  const now = useNow();
  const queryClient = useQueryClient();
  const create = useCreateApiToken();
  const { reset: resetCreate } = create;
  const nameId = useId();
  const nameHintId = useId();
  const nameErrorId = useId();
  const scopesErrorId = useId();
  const endpointsErrorId = useId();
  const expiresId = useId();
  const nameRef = useRef<HTMLInputElement>(null);
  const formRef = useRef<HTMLFormElement>(null);
  const goBackRef = useRef<HTMLButtonElement>(null);
  const submittingRef = useRef(false);
  const abortRef = useRef<AbortController | null>(null);
  const [draft, setDraft] = useState<TokenDraft>(emptyDraft);
  const [submitted, setSubmitted] = useState(false);
  const [failure, setFailure] = useState<TokenErrorInfo | null>(null);
  const [revealed, setRevealed] = useState<RevealedToken | null>(null);
  const [copied, setCopied] = useState(false);
  const [confirmingClose, setConfirmingClose] = useState(false);

  useEffect(() => {
    return () => {
      resetCreate();
    };
  }, [resetCreate]);

  useEffect(() => {
    if (confirmingClose) {
      goBackRef.current?.focus();
    }
  }, [confirmingClose]);

  const owned = useOwnedEndpoints(draft.endpointMode === 'only');
  const knownIds = new Set(flattenOwnedEndpoints(owned.data).map((endpoint) => endpoint.id));
  const effective = owned.data === undefined ? draft : pruneEndpointIds(draft, knownIds);
  const problems = validateDraft(effective);
  const shownProblems = submitted ? problems : {};
  const busy = create.isPending;

  function requestClose() {
    if (busy) {
      abortRef.current?.abort();
      return;
    }
    if (revealed !== null && !copied) {
      setConfirmingClose(true);
      return;
    }
    onClose();
  }

  function focusFirstProblem() {
    if (problems.name !== undefined) {
      nameRef.current?.focus();
      return;
    }
    const selector = problems.scopes !== undefined ? 'fieldset input[type="checkbox"]' : '[data-picker] input';
    formRef.current?.querySelector<HTMLElement>(selector)?.focus();
  }

  async function submit(event: FormEvent<HTMLFormElement>) {
    event.preventDefault();
    if (submittingRef.current) {
      return;
    }
    setSubmitted(true);
    if (hasProblems(problems)) {
      focusFirstProblem();
      return;
    }
    setFailure(null);
    submittingRef.current = true;
    const controller = new AbortController();
    abortRef.current = controller;
    try {
      const created = await create.mutateAsync({ input: draftToInput(effective), cancel: controller.signal });
      setRevealed({ secret: created.token, name: created.name });
    } catch (error) {
      setFailure(describeTokenError(error));
    } finally {
      submittingRef.current = false;
      abortRef.current = null;
      resetCreate();
    }
  }

  function checkList() {
    void queryClient.invalidateQueries({ queryKey: tokenQueryKeys.list() });
    onClose();
  }

  function expiryLabel(days: number): string {
    return days === YEAR_DAYS ? t('wl.tokens.expiry.year') : t('wl.tokens.expiry.days', { count: days });
  }

  const title = revealed === null ? t('wl.tokens.dialog.createTitle') : t('wl.tokens.secret.title');
  const description =
    revealed === null
      ? t('wl.tokens.dialog.createDescription')
      : t('wl.tokens.secret.description', { name: revealed.name });

  return (
    <Dialog.Root
      open
      onOpenChange={(open) => {
        if (!open) {
          requestClose();
        }
      }}
    >
      <Dialog.Portal>
        <Dialog.Overlay className={styles.overlay} />
        <Dialog.Content className={styles.content}>
          <Dialog.Title className={styles.title}>{title}</Dialog.Title>
          <Dialog.Description className={styles.description}>{description}</Dialog.Description>
          {revealed === null ? (
            <form ref={formRef} className={styles.form} noValidate onSubmit={(event) => void submit(event)}>
              <div className={styles.field}>
                <label htmlFor={nameId} className={styles.label}>
                  {t('wl.tokens.field.name')}
                </label>
                <input
                  id={nameId}
                  ref={nameRef}
                  className={styles.input}
                  type="text"
                  autoComplete="off"
                  spellCheck={false}
                  value={draft.name}
                  aria-invalid={shownProblems.name !== undefined}
                  aria-describedby={shownProblems.name === undefined ? nameHintId : `${nameHintId} ${nameErrorId}`}
                  onChange={(event) => setDraft({ ...draft, name: event.target.value })}
                />
                <p id={nameHintId} className={styles.hint}>
                  {t('wl.tokens.field.nameHint')}
                </p>
                {shownProblems.name === 'required' && (
                  <p id={nameErrorId} className={styles.error}>
                    {t('wl.tokens.validation.nameRequired')}
                  </p>
                )}
                {shownProblems.name === 'tooLong' && (
                  <p id={nameErrorId} className={styles.error}>
                    {t('wl.tokens.validation.nameTooLong')}
                  </p>
                )}
                {shownProblems.name === 'control' && (
                  <p id={nameErrorId} className={styles.error}>
                    {t('wl.tokens.validation.nameControl')}
                  </p>
                )}
              </div>

              <div className={styles.field}>
                <ScopeGrid
                  value={draft.scopes}
                  describedBy={shownProblems.scopes === undefined ? undefined : scopesErrorId}
                  onToggle={(scope, on) => setDraft({ ...draft, scopes: toggleScope(draft.scopes, scope, on) })}
                />
                {writesEndpointsWhileRestricted(draft) && (
                  <p className={styles.hint}>{t('wl.tokens.grid.restrictedWrite')}</p>
                )}
                {shownProblems.scopes !== undefined && (
                  <p id={scopesErrorId} className={styles.error}>
                    {t('wl.tokens.validation.scopesRequired')}
                  </p>
                )}
              </div>

              <fieldset className={styles.field}>
                <legend className={styles.label}>{t('wl.tokens.restrict.legend')}</legend>
                <label className={styles.choice}>
                  <input
                    type="radio"
                    name={`${nameId}-mode`}
                    className={styles.radio}
                    checked={draft.endpointMode === 'all'}
                    onChange={() => setDraft({ ...draft, endpointMode: 'all' })}
                  />
                  {t('wl.tokens.restrict.all')}
                </label>
                <label className={styles.choice}>
                  <input
                    type="radio"
                    name={`${nameId}-mode`}
                    className={styles.radio}
                    checked={draft.endpointMode === 'only'}
                    onChange={() => setDraft({ ...draft, endpointMode: 'only' })}
                  />
                  {t('wl.tokens.restrict.only')}
                </label>
                {draft.endpointMode === 'only' && (
                  <div data-picker="">
                    <EndpointPicker
                      selected={effective.endpointIds}
                      describedBy={shownProblems.endpoints === undefined ? undefined : endpointsErrorId}
                      onToggle={(endpointId, on) =>
                        setDraft({ ...draft, endpointIds: toggleEndpoint(draft.endpointIds, endpointId, on) })
                      }
                    />
                  </div>
                )}
                {shownProblems.endpoints !== undefined && (
                  <p id={endpointsErrorId} className={styles.error}>
                    {t('wl.tokens.validation.endpointsRequired')}
                  </p>
                )}
              </fieldset>

              <div className={styles.field}>
                <label htmlFor={expiresId} className={styles.label}>
                  {t('wl.tokens.field.expires')}
                </label>
                <select
                  id={expiresId}
                  className={styles.input}
                  value={draft.days}
                  onChange={(event) => setDraft({ ...draft, days: Number(event.target.value) })}
                >
                  {EXPIRY_PRESETS.map((days) => (
                    <option key={days} value={days}>
                      {expiryLabel(days)}
                    </option>
                  ))}
                </select>
                <p className={styles.hint}>
                  {t('wl.tokens.field.expiresOn', {
                    date: formatDate(expiryDate(draft.days, now).toISOString(), language),
                  })}
                </p>
              </div>

              {failure !== null && <TokenErrorNotice error={failure} />}
              {failure?.maybeCreated === true && (
                <div role="alert" className={styles.confirm}>
                  <p className={styles.confirmTitle}>{t('wl.tokens.error.maybeCreated')}</p>
                  <div className={styles.actions}>
                    <button type="button" className={styles.secondary} onClick={checkList}>
                      {t('wl.tokens.error.checkList')}
                    </button>
                  </div>
                </div>
              )}

              <div className={styles.actions}>
                <button type="button" className={styles.secondary} onClick={requestClose}>
                  {t('wl.tokens.dialog.cancel')}
                </button>
                <button type="submit" className={styles.primary} disabled={busy}>
                  {busy ? t('wl.tokens.dialog.busy') : t('wl.tokens.dialog.submit')}
                </button>
              </div>
            </form>
          ) : (
            <div className={styles.form}>
              <SecretReveal secret={revealed.secret} onCopied={() => setCopied(true)} />
              {confirmingClose ? (
                <div role="alert" className={styles.confirm}>
                  <p className={styles.confirmTitle}>
                    {t('wl.tokens.secret.closeTitle')}{' '}
                    <span className={styles.hint}>{t('wl.tokens.secret.closeBody')}</span>
                  </p>
                  <div className={styles.actions}>
                    <button
                      ref={goBackRef}
                      type="button"
                      className={styles.secondary}
                      onClick={() => setConfirmingClose(false)}
                    >
                      {t('wl.tokens.secret.goBack')}
                    </button>
                    <button type="button" className={styles.danger} onClick={onClose}>
                      {t('wl.tokens.secret.closeAnyway')}
                    </button>
                  </div>
                </div>
              ) : (
                <div className={styles.actions}>
                  <button type="button" className={styles.primary} onClick={requestClose}>
                    {t('wl.tokens.secret.done')}
                  </button>
                </div>
              )}
            </div>
          )}
        </Dialog.Content>
      </Dialog.Portal>
    </Dialog.Root>
  );
}
