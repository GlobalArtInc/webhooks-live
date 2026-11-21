import { Link } from '@tanstack/react-router';
import { useId, type Ref } from 'react';
import { useTranslation } from 'react-i18next';
import styles from './RuleEditor.module.css';

export interface EditorHeaderProps {
  endpointId: string;
  creating: boolean;
  name: string;
  nameMessages: readonly string[];
  readOnly: boolean;
  onNameChange: (name: string) => void;
  enabled: boolean | null;
  toggleBusy: boolean;
  onToggle: () => void;
  nameRef: Ref<HTMLInputElement>;
}

export function EditorHeader({
  endpointId,
  creating,
  name,
  nameMessages,
  readOnly,
  onNameChange,
  enabled,
  toggleBusy,
  onToggle,
  nameRef,
}: EditorHeaderProps) {
  const { t } = useTranslation();
  const inputId = useId();
  const errorId = useId();
  const invalid = nameMessages.length > 0;
  const switchClass = enabled === true ? `${styles.switch} ${styles.switchOn}` : styles.switch;

  return (
    <header className={styles.header}>
      <Link to="/e/$id/settings/$section" params={{ id: endpointId, section: 'rules' }} className={styles.back}>
        {t('wl.ruleeditor.back')}
      </Link>
      <h2 className={styles.title}>{creating ? t('wl.ruleeditor.titleNew') : t('wl.ruleeditor.title')}</h2>
      <div className={styles.identity}>
        <div className={styles.nameField}>
          <label htmlFor={inputId} className={styles.label}>
            {t('wl.ruleeditor.name.label')}
          </label>
          <input
            id={inputId}
            ref={nameRef}
            type="text"
            className={styles.input}
            value={name}
            readOnly={readOnly}
            autoComplete="off"
            spellCheck={false}
            aria-invalid={invalid}
            aria-describedby={invalid ? errorId : undefined}
            onChange={(event) => {
              onNameChange(event.target.value);
            }}
          />
          {invalid ? (
            <p id={errorId} className={styles.fieldError}>
              {nameMessages.join(' ')}
            </p>
          ) : null}
        </div>
        {enabled === null ? null : (
          <button
            type="button"
            role="switch"
            aria-checked={enabled}
            aria-label={t('wl.ruleeditor.enabled.label')}
            className={switchClass}
            disabled={toggleBusy}
            onClick={onToggle}
          >
            <span className={styles.knob} aria-hidden="true" />
            <span>{enabled ? t('wl.rules.state.on') : t('wl.rules.state.off')}</span>
          </button>
        )}
      </div>
    </header>
  );
}
