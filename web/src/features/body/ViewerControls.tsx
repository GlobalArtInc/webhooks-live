import type { KeyboardEvent } from 'react';
import styles from './ViewerControls.module.css';
import type { BodyViewMode } from './types';

interface ViewerControlsProps {
  showMode: boolean;
  mode: BodyViewMode;
  prettyDisabled: boolean;
  onModeChange: (mode: BodyViewMode) => void;
  showWrap: boolean;
  wrap: boolean;
  onWrapChange: (wrap: boolean) => void;
  showSearch: boolean;
  search: string;
  onSearchChange: (search: string) => void;
  onFindNext: () => void;
  onFindPrevious: () => void;
  onDownloadRaw: () => void;
}

export function ViewerControls({
  showMode,
  mode,
  prettyDisabled,
  onModeChange,
  showWrap,
  wrap,
  onWrapChange,
  showSearch,
  search,
  onSearchChange,
  onFindNext,
  onFindPrevious,
  onDownloadRaw,
}: ViewerControlsProps) {
  const handleSearchKey = (event: KeyboardEvent<HTMLInputElement>): void => {
    if (event.key !== 'Enter') {
      return;
    }
    event.preventDefault();
    if (event.shiftKey) {
      onFindPrevious();
    } else {
      onFindNext();
    }
  };

  return (
    <div className={styles.controls} role="toolbar" aria-label="Body viewer controls">
      {showMode ? (
        <div className={styles.group} role="group" aria-label="Display mode">
          <button
            type="button"
            className={styles.segment}
            aria-pressed={mode === 'pretty'}
            disabled={prettyDisabled}
            onClick={() => {
              onModeChange('pretty');
            }}
          >
            Pretty
          </button>
          <button
            type="button"
            className={styles.segment}
            aria-pressed={mode === 'raw'}
            onClick={() => {
              onModeChange('raw');
            }}
          >
            Raw
          </button>
        </div>
      ) : null}
      {showWrap ? (
        <button
          type="button"
          className={styles.button}
          aria-pressed={wrap}
          onClick={() => {
            onWrapChange(!wrap);
          }}
        >
          Wrap
        </button>
      ) : null}
      {showSearch ? (
        <div className={styles.search}>
          <input
            type="search"
            className={styles.input}
            aria-label="Search in body"
            placeholder="Search in body"
            value={search}
            onChange={(event) => {
              onSearchChange(event.currentTarget.value);
            }}
            onKeyDown={handleSearchKey}
          />
          <button type="button" className={styles.button} disabled={search === ''} onClick={onFindPrevious}>
            Previous match
          </button>
          <button type="button" className={styles.button} disabled={search === ''} onClick={onFindNext}>
            Next match
          </button>
        </div>
      ) : null}
      <button type="button" className={styles.button} onClick={onDownloadRaw}>
        Download raw
      </button>
    </div>
  );
}
