import { useEffect, useId, useRef, useState } from 'react';
import { useTranslation } from 'react-i18next';
import { CopyButton } from '../../components/CopyButton/CopyButton';
import { SECRET_MAX_BYTES, generateSecret } from './secrets';
import { useAccessIssueText } from './issueText';
import type { AccessDraft, AccessIssue, AuthKindChoice, CaptureAuthView } from './types';
import styles from './access.module.css';

export interface CaptureAuthSettingProps {
  draft: AccessDraft;
  current: CaptureAuthView | null;
  readOnly: boolean;
  nameIssue: AccessIssue | null;
  secretIssue: AccessIssue | null;
  onChange: (draft: AccessDraft) => void;
}

const KINDS: readonly AuthKindChoice[] = ['none', 'basic', 'header'];

export function CaptureAuthSetting({
  draft,
  current,
  readOnly,
  nameIssue,
  secretIssue,
  onChange,
}: CaptureAuthSettingProps) {
  const { t } = useTranslation();
  const issueText = useAccessIssueText();
  const baseId = useId();
  const secretRef = useRef<HTMLInputElement>(null);
  const [reveal, setReveal] = useState(false);
  const [generated, setGenerated] = useState(false);
  const [focusSecret, setFocusSecret] = useState(0);
  const nameId = `${baseId}-name`;
  const secretId = `${baseId}-secret`;
  const nameErrorId = `${baseId}-name-error`;
  const secretErrorId = `${baseId}-secret-error`;
  const onceId = `${baseId}-once`;
  const basic = draft.authKind === 'basic';
  const secretEmpty = draft.authSecret === '';
  const [wasEmpty, setWasEmpty] = useState(secretEmpty);
  if (secretEmpty !== wasEmpty) {
    setWasEmpty(secretEmpty);
    if (secretEmpty) {
      setReveal(false);
      setGenerated(false);
    }
  }
  const showOnce = generated && draft.authSecret !== '';
  const secretShown = reveal || showOnce;
  const keepsSecret = current !== null && current.kind === draft.authKind;

  useEffect(() => {
    if (focusSecret > 0) {
      secretRef.current?.focus();
    }
  }, [focusSecret]);

  function kindLabel(kind: AuthKindChoice): string {
    switch (kind) {
      case 'none':
        return t('wl.access.captureAuth.kind.none');
      case 'basic':
        return t('wl.access.captureAuth.kind.basic');
      case 'header':
        return t('wl.access.captureAuth.kind.header');
    }
  }

  function chooseKind(kind: AuthKindChoice): void {
    setReveal(false);
    setGenerated(false);
    const keepsName = current !== null && current.kind === kind;
    onChange({ ...draft, authKind: kind, authName: keepsName ? current.name : '', authSecret: '' });
  }

  function generate(): void {
    setGenerated(true);
    setFocusSecret((count) => count + 1);
    onChange({ ...draft, authSecret: generateSecret() });
  }

  const currentText =
    current === null
      ? t('wl.access.captureAuth.currentNone')
      : current.kind === 'basic'
        ? t('wl.access.captureAuth.currentBasic', { name: current.name })
        : t('wl.access.captureAuth.currentHeader', { name: current.name });

  return (
    <fieldset className={styles.block} disabled={readOnly}>
      <legend className={styles.blockTitle}>{t('wl.access.captureAuth.title')}</legend>
      <p className={styles.hint}>{t('wl.access.captureAuth.description')}</p>
      <p className={styles.status}>{currentText}</p>
      <div className={styles.radios} role="radiogroup" aria-label={t('wl.access.captureAuth.kind.label')}>
        {KINDS.map((kind) => (
          <label key={kind} className={styles.radio}>
            <input
              type="radio"
              name={`${baseId}-kind`}
              checked={draft.authKind === kind}
              onChange={() => {
                chooseKind(kind);
              }}
            />
            {kindLabel(kind)}
          </label>
        ))}
      </div>
      {draft.authKind === 'none' ? null : (
        <div className={styles.fields}>
          <div className={styles.field}>
            <label htmlFor={nameId} className={styles.label}>
              {basic ? t('wl.access.captureAuth.username') : t('wl.access.captureAuth.headerName')}
            </label>
            <input
              id={nameId}
              type="text"
              className={styles.input}
              autoComplete="off"
              autoCapitalize="off"
              spellCheck={false}
              maxLength={SECRET_MAX_BYTES + 1}
              value={draft.authName}
              aria-invalid={nameIssue !== null ? true : undefined}
              aria-describedby={nameIssue !== null ? nameErrorId : undefined}
              onChange={(event) => {
                onChange({ ...draft, authName: event.target.value });
              }}
            />
            {nameIssue === null ? null : (
              <p id={nameErrorId} role="alert" className={styles.error}>
                {issueText(nameIssue)}
              </p>
            )}
          </div>
          <div className={styles.field}>
            <label htmlFor={secretId} className={styles.label}>
              {basic ? t('wl.access.captureAuth.password') : t('wl.access.captureAuth.headerValue')}
            </label>
            <div className={styles.inputRow}>
              <input
                ref={secretRef}
                id={secretId}
                type={secretShown ? 'text' : 'password'}
                className={styles.input}
                autoComplete="new-password"
                autoCapitalize="off"
                spellCheck={false}
                maxLength={SECRET_MAX_BYTES + 1}
                placeholder={keepsSecret ? t('wl.access.captureAuth.unchanged') : undefined}
                value={draft.authSecret}
                aria-invalid={secretIssue !== null ? true : undefined}
                aria-describedby={
                  [showOnce ? onceId : null, secretIssue !== null ? secretErrorId : null]
                    .filter((id): id is string => id !== null)
                    .join(' ') || undefined
                }
                onChange={(event) => {
                  setGenerated(false);
                  onChange({ ...draft, authSecret: event.target.value });
                }}
              />
              {readOnly ? null : (
                <>
                  <button type="button" className={styles.button} onClick={generate}>
                    {t('wl.access.captureAuth.generate')}
                  </button>
                  <button
                    type="button"
                    className={styles.button}
                    aria-pressed={secretShown}
                    disabled={showOnce}
                    onClick={() => {
                      setReveal((value) => !value);
                    }}
                  >
                    {secretShown ? t('wl.access.captureAuth.hide') : t('wl.access.captureAuth.show')}
                  </button>
                  {showOnce ? <CopyButton value={draft.authSecret} label={t('wl.access.captureAuth.copy')} /> : null}
                </>
              )}
            </div>
            {showOnce ? (
              <p id={onceId} className={styles.secret}>
                {t('wl.access.captureAuth.once')}
              </p>
            ) : null}
            {secretIssue === null ? null : (
              <p id={secretErrorId} role="alert" className={styles.error}>
                {issueText(secretIssue)}
              </p>
            )}
            <p className={styles.hint}>{t('wl.access.captureAuth.neverShown')}</p>
          </div>
        </div>
      )}
    </fieldset>
  );
}
