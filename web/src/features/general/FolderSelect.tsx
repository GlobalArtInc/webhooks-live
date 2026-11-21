import { useId } from 'react';
import { useTranslation } from 'react-i18next';
import type { Folder } from '../../api/types';
import styles from './FolderSelect.module.css';

const UNFILED_VALUE = '';

export interface FolderSelectProps {
  folders: readonly Folder[];
  value: string | null;
  disabled: boolean;
  onChange: (folderId: string | null) => void;
}

export function FolderSelect({ folders, value, disabled, onChange }: FolderSelectProps) {
  const { t } = useTranslation();
  const selectId = useId();
  return (
    <div className={styles.field}>
      <label htmlFor={selectId} className={styles.label}>
        {t('wl.general.folder.label')}
      </label>
      <select
        id={selectId}
        className={styles.select}
        value={value ?? UNFILED_VALUE}
        disabled={disabled}
        onChange={(event) => {
          onChange(event.target.value === UNFILED_VALUE ? null : event.target.value);
        }}
      >
        <option value={UNFILED_VALUE}>{t('wl.general.folder.unfiled')}</option>
        {folders.map((folder) => (
          <option key={folder.id} value={folder.id}>
            {folder.name}
          </option>
        ))}
      </select>
    </div>
  );
}
