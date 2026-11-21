import { useId } from 'react';
import { useTranslation } from 'react-i18next';
import { ChipInput } from '../../components/ChipInput/ChipInput';
import { cidrForAddress, isCovered, normalizeCidr, parseCidr } from './cidr';
import { useAccessIssueText } from './issueText';
import type { AccessDraft, AccessIssue } from './types';
import styles from './access.module.css';

export const MAX_ALLOW_CIDRS = 20;
const MAX_CIDR_LENGTH = 45 + 4;

export interface SendersSettingProps {
  draft: AccessDraft;
  clientIp: string | null;
  readOnly: boolean;
  issue: AccessIssue | null;
  onChange: (draft: AccessDraft) => void;
}

export function SendersSetting({ draft, clientIp, readOnly, issue, onChange }: SendersSettingProps) {
  const { t } = useTranslation();
  const issueText = useAccessIssueText();
  const baseId = useId();
  const errorId = `${baseId}-error`;
  const ownCidr = clientIp === null ? null : cidrForAddress(clientIp);
  const outside = draft.sendersEnabled && clientIp !== null && ownCidr !== null && !isCovered(draft.cidrs, clientIp);

  function validate(value: string): string | null {
    const parsed = parseCidr(value);
    if (parsed.ok) {
      return null;
    }
    switch (parsed.problem) {
      case 'format':
        return t('wl.access.allowList.error.format');
      case 'prefix':
        return t('wl.access.allowList.error.prefix');
      case 'hostBits':
        return t('wl.access.allowList.error.hostBits', { suggestion: parsed.suggestion ?? '' });
    }
  }

  return (
    <fieldset className={styles.block} disabled={readOnly}>
      <legend className={styles.blockTitle}>{t('wl.access.allowList.title')}</legend>
      <label className={styles.check}>
        <input
          type="checkbox"
          checked={draft.sendersEnabled}
          onChange={(event) => {
            onChange({ ...draft, sendersEnabled: event.target.checked });
          }}
        />
        {t('wl.access.allowList.enable')}
      </label>
      <p className={styles.hint}>{t('wl.access.allowList.description')}</p>
      {draft.sendersEnabled ? (
        <>
          <ChipInput
            label={t('wl.access.allowList.label')}
            values={draft.cidrs}
            onChange={(cidrs) => {
              onChange({ ...draft, cidrs });
            }}
            validate={validate}
            normalize={normalizeCidr}
            maxItems={MAX_ALLOW_CIDRS}
            maxLength={MAX_CIDR_LENGTH}
            placeholder={t('wl.access.allowList.placeholder')}
            hint={t('wl.access.allowList.hint', { max: MAX_ALLOW_CIDRS })}
            readOnly={readOnly}
          />
          {issue === null ? null : (
            <p id={errorId} role="alert" className={styles.error}>
              {issueText(issue)}
            </p>
          )}
          {outside && ownCidr !== null && !readOnly ? (
            <p className={styles.notice}>
              <span>{t('wl.access.allowList.outside', { address: clientIp })}</span>
              <button
                type="button"
                className={styles.button}
                disabled={draft.cidrs.length >= MAX_ALLOW_CIDRS}
                onClick={() => {
                  onChange({ ...draft, cidrs: [...draft.cidrs, ownCidr] });
                }}
              >
                {t('wl.access.allowList.addOwn')}
              </button>
            </p>
          ) : null}
        </>
      ) : null}
    </fieldset>
  );
}
