import { useId, useState, type FormEvent, type KeyboardEvent } from 'react';
import { useTranslation } from 'react-i18next';
import { MAX_FOLDER_NAME_LENGTH, normalizeFolderName } from './localOrganize';
import { folderNameProblem, type FolderSource, type NameProblem } from './model';
import styles from './FolderForm.module.css';

export interface FolderFormProps {
  initialName: string;
  initialSource: FolderSource;
  showSource: boolean;
  submitLabel: string;
  busy: boolean;
  takenNames: (source: FolderSource) => readonly string[];
  onSubmit: (name: string, source: FolderSource) => void;
  onCancel: () => void;
}

export function FolderForm({
  initialName,
  initialSource,
  showSource,
  submitLabel,
  busy,
  takenNames,
  onSubmit,
  onCancel,
}: FolderFormProps) {
  const { t } = useTranslation();
  const baseId = useId();
  const [name, setName] = useState(initialName);
  const [source, setSource] = useState<FolderSource>(initialSource);
  const [problem, setProblem] = useState<NameProblem | null>(null);
  const errorId = `${baseId}-error`;

  function handleSubmit(event: FormEvent<HTMLFormElement>) {
    event.preventDefault();
    const nextProblem = folderNameProblem(name, takenNames(source));
    setProblem(nextProblem);
    if (nextProblem === null) {
      onSubmit(normalizeFolderName(name), source);
    }
  }

  function handleKeyDown(event: KeyboardEvent<HTMLInputElement>) {
    if (event.key === 'Escape') {
      event.preventDefault();
      onCancel();
    }
  }

  function problemText(current: NameProblem): string {
    switch (current) {
      case 'empty':
        return t('wl.rail.folders.nameError.empty');
      case 'tooLong':
        return t('wl.rail.folders.nameError.tooLong', { max: MAX_FOLDER_NAME_LENGTH });
      case 'duplicate':
        return t('wl.rail.folders.nameError.duplicate');
    }
  }

  return (
    <form className={styles.form} onSubmit={handleSubmit} noValidate>
      <label className={styles.field}>
        <span className={styles.label}>{t('wl.rail.folders.nameLabel')}</span>
        <input
          type="text"
          className={styles.input}
          value={name}
          autoFocus
          aria-invalid={problem !== null}
          aria-describedby={problem === null ? undefined : errorId}
          onChange={(event) => {
            setName(event.target.value);
            setProblem(null);
          }}
          onKeyDown={handleKeyDown}
        />
      </label>
      {showSource ? (
        <label className={styles.field}>
          <span className={styles.label}>{t('wl.rail.folders.storeLabel')}</span>
          <select
            className={styles.input}
            value={source}
            onChange={(event) => setSource(event.target.value === 'local' ? 'local' : 'account')}
          >
            <option value="account">{t('wl.rail.source.account')}</option>
            <option value="local">{t('wl.rail.source.local')}</option>
          </select>
        </label>
      ) : null}
      {problem === null ? null : (
        <p id={errorId} className={styles.error} role="alert">
          {problemText(problem)}
        </p>
      )}
      <div className={styles.actions}>
        <button type="submit" className={styles.submit} disabled={busy}>
          {submitLabel}
        </button>
        <button type="button" className={styles.cancel} onClick={onCancel}>
          {t('wl.rail.folders.cancel')}
        </button>
      </div>
    </form>
  );
}
