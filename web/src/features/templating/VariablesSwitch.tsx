import { useId } from 'react';
import { useTranslation } from 'react-i18next';
import styles from './VariablesSwitch.module.css';

export interface VariablesSwitchProps {
  checked: boolean;
  onChange: (checked: boolean) => void;
  disabled?: boolean;
}

export function VariablesSwitch({ checked, onChange, disabled = false }: VariablesSwitchProps) {
  const { t } = useTranslation();
  const labelId = useId();
  const hintId = useId();
  return (
    <div className={styles.row}>
      <span id={labelId} className={styles.label}>
        {t('wl.templating.useVariables')}
      </span>
      <button
        type="button"
        role="switch"
        aria-checked={checked}
        aria-labelledby={labelId}
        aria-describedby={hintId}
        className={checked ? `${styles.switch} ${styles.on}` : styles.switch}
        disabled={disabled}
        onClick={() => {
          onChange(!checked);
        }}
      >
        <span className={styles.knob} aria-hidden="true" />
        <span>{checked ? t('wl.templating.switch.on') : t('wl.templating.switch.off')}</span>
      </button>
      <p id={hintId} className={styles.hint}>
        {t('wl.templating.useVariablesHint')}
      </p>
    </div>
  );
}
