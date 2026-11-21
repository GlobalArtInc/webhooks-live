import { useId, useState, type FormEvent } from 'react';
import { useTranslation } from 'react-i18next';
import {
  addSavedFilter,
  cleanName,
  deleteSavedFilter,
  loadSavedFilters,
  MAX_SAVED_FILTERS,
  MAX_SAVED_NAME_LENGTH,
  newSavedFilterId,
  renameSavedFilter,
  storeSavedFilters,
  type SavedFilter,
} from './savedFilters';
import styles from './FilterBuilder.module.css';

export interface SavedFiltersProps {
  endpointId: string;
  filterFor: () => string | null;
  onOpen: (filter: string) => void;
}

export function SavedFilters({ endpointId, filterFor, onOpen }: SavedFiltersProps) {
  const { t } = useTranslation();
  const headingId = useId();
  const [filters, setFilters] = useState<SavedFilter[]>(() => loadSavedFilters(endpointId));
  const [name, setName] = useState('');
  const [renamingId, setRenamingId] = useState<string | null>(null);
  const [renameText, setRenameText] = useState('');
  const [storageFailed, setStorageFailed] = useState(false);
  const full = filters.length >= MAX_SAVED_FILTERS;

  function commit(next: SavedFilter[]) {
    setFilters(next);
    setStorageFailed(!storeSavedFilters(endpointId, next));
  }

  function save(event: FormEvent<HTMLFormElement>) {
    event.preventDefault();
    const cleaned = cleanName(name);
    const filter = filterFor();
    if (cleaned === undefined || filter === null || full) {
      return;
    }
    commit(addSavedFilter(filters, { id: newSavedFilterId(), name: cleaned, filter }));
    setName('');
  }

  function saveRename(event: FormEvent<HTMLFormElement>, id: string) {
    event.preventDefault();
    const cleaned = cleanName(renameText);
    if (cleaned !== undefined) {
      commit(renameSavedFilter(filters, id, cleaned));
    }
    setRenamingId(null);
  }

  return (
    <section className={styles.saved} aria-labelledby={headingId}>
      <h3 id={headingId} className={styles.sectionTitle}>
        {t('wl.filterbuilder.saved.title')}
      </h3>
      {filters.length === 0 ? (
        <p className={styles.hint}>{t('wl.filterbuilder.saved.empty')}</p>
      ) : (
        <ul className={styles.savedList}>
          {filters.map((item) =>
            renamingId === item.id ? (
              <li key={item.id} className={styles.savedItem}>
                <form
                  className={styles.inlineForm}
                  onSubmit={(event) => {
                    saveRename(event, item.id);
                  }}
                >
                  <input
                    className={styles.input}
                    aria-label={t('wl.filterbuilder.saved.nameLabel')}
                    value={renameText}
                    maxLength={MAX_SAVED_NAME_LENGTH}
                    onChange={(event) => {
                      setRenameText(event.target.value);
                    }}
                  />
                  <button type="submit" className={styles.secondary} disabled={cleanName(renameText) === undefined}>
                    {t('wl.filterbuilder.saved.renameSubmit')}
                  </button>
                  <button
                    type="button"
                    className={styles.secondary}
                    onClick={() => {
                      setRenamingId(null);
                    }}
                  >
                    {t('wl.filterbuilder.saved.cancel')}
                  </button>
                </form>
              </li>
            ) : (
              <li key={item.id} className={styles.savedItem}>
                <button
                  type="button"
                  className={styles.savedName}
                  aria-label={t('wl.filterbuilder.saved.apply', { name: item.name })}
                  onClick={() => {
                    onOpen(item.filter);
                  }}
                >
                  {item.name}
                </button>
                <button
                  type="button"
                  className={styles.secondary}
                  aria-label={t('wl.filterbuilder.saved.renameNamed', { name: item.name })}
                  onClick={() => {
                    setRenamingId(item.id);
                    setRenameText(item.name);
                  }}
                >
                  {t('wl.filterbuilder.saved.rename')}
                </button>
                <button
                  type="button"
                  className={styles.secondary}
                  aria-label={t('wl.filterbuilder.saved.deleteNamed', { name: item.name })}
                  onClick={() => {
                    commit(deleteSavedFilter(filters, item.id));
                  }}
                >
                  {t('wl.filterbuilder.saved.delete')}
                </button>
              </li>
            ),
          )}
        </ul>
      )}
      <form className={styles.inlineForm} onSubmit={save}>
        <label className={styles.grow}>
          <span className={styles.label}>{t('wl.filterbuilder.saved.nameLabel')}</span>
          <input
            className={styles.input}
            value={name}
            maxLength={MAX_SAVED_NAME_LENGTH}
            disabled={full}
            onChange={(event) => {
              setName(event.target.value);
            }}
          />
        </label>
        <button type="submit" className={styles.secondary} disabled={full || cleanName(name) === undefined}>
          {t('wl.filterbuilder.saved.save')}
        </button>
      </form>
      {full ? <p className={styles.hint}>{t('wl.filterbuilder.saved.full', { max: MAX_SAVED_FILTERS })}</p> : null}
      {storageFailed ? (
        <p role="status" className={styles.error}>
          {t('wl.filterbuilder.saved.storageFailed')}
        </p>
      ) : null}
    </section>
  );
}
