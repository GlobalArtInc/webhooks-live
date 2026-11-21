import { useId, useState, type FormEvent, type KeyboardEvent } from 'react';
import { useTranslation } from 'react-i18next';
import { MAX_LABEL_LENGTH } from './storage';
import styles from './EndpointLabelEditor.module.css';

export interface EndpointLabelEditorProps {
  name: string;
  initial: string;
  onSave: (label: string) => void;
  onCancel: () => void;
}

export function EndpointLabelEditor({ name, initial, onSave, onCancel }: EndpointLabelEditorProps) {
  const { t } = useTranslation();
  const [value, setValue] = useState(initial);
  const hintId = useId();

  function handleSubmit(event: FormEvent<HTMLFormElement>) {
    event.preventDefault();
    onSave(value);
  }

  function handleKeyDown(event: KeyboardEvent<HTMLInputElement>) {
    if (event.key === 'Escape') {
      event.preventDefault();
      onCancel();
    }
  }

  return (
    <form className={styles.editor} onSubmit={handleSubmit}>
      <input
        type="text"
        className={styles.input}
        aria-label={t('wl.home.list.labelFor', { name })}
        aria-describedby={hintId}
        autoFocus
        maxLength={MAX_LABEL_LENGTH}
        value={value}
        onChange={(event) => setValue(event.target.value)}
        onKeyDown={handleKeyDown}
      />
      <span id={hintId} className={styles.hint}>
        {t('wl.home.list.labelHint', { max: MAX_LABEL_LENGTH })}
      </span>
      <div className={styles.actions}>
        <button type="submit" className={styles.save}>
          {t('wl.home.list.save')}
        </button>
        <button type="button" className={styles.cancel} onClick={onCancel}>
          {t('wl.home.list.cancel')}
        </button>
      </div>
    </form>
  );
}
