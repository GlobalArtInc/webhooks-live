import * as Dialog from '@radix-ui/react-dialog';
import { useState } from 'react';
import { useTranslation } from 'react-i18next';
import type { ExportFormat } from '../../api/types';
import { formatByteSize } from '../../components/format/format';
import { useReturnFocus } from '../../components/useReturnFocus';
import type { ActiveFilters } from '../list/listFilters';
import { EXPORT_MAX_CAPTURES, exportQueryOf, selectionFitsExport, type ExportScope } from './exportRequest';
import { useExportJob, type ExportJob, type ExportPhase } from './useExportJob';
import styles from './ExportDialog.module.css';

export interface ExportCounts {
  filtered: number | null;
  all: number;
}

export interface ExportSizes {
  selected: number;
  all: number;
}

export interface ExportDialogProps {
  endpointId: string;
  initialScope: ExportScope;
  selectedIds: readonly string[];
  filters: ActiveFilters;
  counts: ExportCounts;
  sizes: ExportSizes;
  onOpenChange: (open: boolean) => void;
}

interface ScopeOption {
  scope: ExportScope;
  label: string;
  count: number | null;
  disabled: boolean;
}

interface FormatOption {
  format: ExportFormat;
  label: string;
  hint: string;
}

export function ExportDialog({
  endpointId,
  initialScope,
  selectedIds,
  filters,
  counts,
  sizes,
  onOpenChange,
}: ExportDialogProps) {
  const { t, i18n } = useTranslation();
  const returnFocus = useReturnFocus();
  const locale = i18n.language;
  const job = useExportJob();
  const [scope, setScope] = useState<ExportScope>(initialScope);
  const [format, setFormat] = useState<ExportFormat>('json');
  const selectedFits = selectionFitsExport(selectedIds.length);

  const scopes: ScopeOption[] = [
    {
      scope: 'selected',
      label: t('wl.exportdialog.scope.selected'),
      count: selectedIds.length,
      disabled: !selectedFits,
    },
    { scope: 'filtered', label: t('wl.exportdialog.scope.filtered'), count: counts.filtered, disabled: false },
    { scope: 'all', label: t('wl.exportdialog.scope.all'), count: counts.all, disabled: false },
  ];
  const formats: FormatOption[] = [
    { format: 'json', label: t('wl.exportdialog.format.json'), hint: t('wl.exportdialog.format.json.hint') },
    { format: 'ndjson', label: t('wl.exportdialog.format.ndjson'), hint: t('wl.exportdialog.format.ndjson.hint') },
    { format: 'csv', label: t('wl.exportdialog.format.csv'), hint: t('wl.exportdialog.format.csv.hint') },
    { format: 'zip', label: t('wl.exportdialog.format.zip'), hint: t('wl.exportdialog.format.zip.hint') },
  ];
  const formatLabel = formats.find((option) => option.format === format)?.label ?? format;
  const sizeBytes = scope === 'selected' ? sizes.selected : scope === 'all' ? sizes.all : null;
  const showsSize = sizeBytes !== null && (scope !== 'selected' || selectedFits);
  const configuring = job.phase.kind === 'idle';

  function start(): void {
    if (scope === 'selected') {
      job.start({ kind: 'ids', endpointId, input: { format, capture_ids: [...selectedIds] } });
      return;
    }
    job.start({
      kind: 'query',
      endpointId,
      query: scope === 'filtered' ? exportQueryOf(format, filters) : { format },
    });
  }

  function handleOpenChange(open: boolean): void {
    if (!open) {
      job.cancel();
    }
    onOpenChange(open);
  }

  return (
    <Dialog.Root open onOpenChange={handleOpenChange}>
      <Dialog.Portal>
        <Dialog.Overlay className={styles.overlay} />
        <Dialog.Content
          className={styles.content}
          onOpenAutoFocus={returnFocus.onOpenAutoFocus}
          onCloseAutoFocus={returnFocus.onCloseAutoFocus}
        >
          <Dialog.Title className={styles.title}>{t('wl.exportdialog.title')}</Dialog.Title>
          <Dialog.Description className={styles.description}>{t('wl.exportdialog.description')}</Dialog.Description>
          {configuring ? (
            <div className={styles.form}>
              <fieldset className={styles.group}>
                <legend className={styles.legend}>{t('wl.exportdialog.scope.legend')}</legend>
                {scopes.map((option) => (
                  <label key={option.scope} className={styles.option}>
                    <input
                      type="radio"
                      name="export-scope"
                      value={option.scope}
                      checked={scope === option.scope}
                      disabled={option.disabled}
                      onChange={() => {
                        setScope(option.scope);
                      }}
                    />
                    <span>{option.label}</span>
                    <span className={styles.count}>{option.count ?? ''}</span>
                  </label>
                ))}
              </fieldset>
              {selectedIds.length > EXPORT_MAX_CAPTURES ? (
                <p className={styles.note}>{t('wl.exportdialog.scope.tooMany', { max: EXPORT_MAX_CAPTURES })}</p>
              ) : null}
              {showsSize ? (
                <p className={styles.note}>
                  {t('wl.exportdialog.scope.sizeHint', { size: formatByteSize(sizeBytes, locale) })}
                </p>
              ) : null}
              <fieldset className={styles.group}>
                <legend className={styles.legend}>{t('wl.exportdialog.format.legend')}</legend>
                {formats.map((option) => (
                  <label key={option.format} className={styles.option}>
                    <input
                      type="radio"
                      name="export-format"
                      value={option.format}
                      checked={format === option.format}
                      onChange={() => {
                        setFormat(option.format);
                      }}
                    />
                    <span>{option.label}</span>
                    <span className={styles.hint}>{option.hint}</span>
                  </label>
                ))}
              </fieldset>
            </div>
          ) : (
            <ExportStatus phase={job.phase} formatLabel={formatLabel} locale={locale} />
          )}
          <div className={styles.actions}>
            <ExportActions
              job={job}
              canStart={scope !== 'selected' || selectedFits}
              onStart={start}
              onClose={() => {
                handleOpenChange(false);
              }}
            />
          </div>
        </Dialog.Content>
      </Dialog.Portal>
    </Dialog.Root>
  );
}

interface ExportStatusProps {
  phase: ExportPhase;
  formatLabel: string;
  locale: string;
}

function ExportStatus({ phase, formatLabel, locale }: ExportStatusProps) {
  const { t } = useTranslation();
  switch (phase.kind) {
    case 'running':
      return (
        <p role="status" aria-busy="true" className={styles.status}>
          {t('wl.exportdialog.running')}
        </p>
      );
    case 'started':
      return (
        <p role="status" className={styles.status}>
          {t('wl.exportdialog.started')}
        </p>
      );
    case 'ready':
      return (
        <p role="status" className={styles.status}>
          {t('wl.exportdialog.ready', { format: formatLabel, size: formatByteSize(phase.size, locale) })}
        </p>
      );
    case 'cancelled':
      return (
        <p role="status" className={styles.status}>
          {t('wl.exportdialog.cancelled')}
        </p>
      );
    case 'failed':
      return (
        <p role="alert" className={styles.failed}>
          {phase.reason === 'busy'
            ? t('wl.exportdialog.failed.busy', { seconds: phase.retryAfter ?? 1 })
            : phase.reason === 'tooLarge'
              ? t('wl.exportdialog.failed.tooLarge')
              : phase.reason === 'gone'
                ? t('wl.exportdialog.failed.gone')
                : t('wl.exportdialog.failed.generic')}
        </p>
      );
    case 'idle':
      return null;
  }
}

interface ExportActionsProps {
  job: ExportJob;
  canStart: boolean;
  onStart: () => void;
  onClose: () => void;
}

function ExportActions({ job, canStart, onStart, onClose }: ExportActionsProps) {
  const { t } = useTranslation();
  const closeButton = (
    <button type="button" className={styles.secondary} onClick={onClose}>
      {t('wl.exportdialog.action.close')}
    </button>
  );
  switch (job.phase.kind) {
    case 'idle':
      return (
        <>
          <button type="button" className={styles.secondary} onClick={onClose}>
            {t('wl.exportdialog.action.cancel')}
          </button>
          <button type="button" className={styles.primary} disabled={!canStart} onClick={onStart}>
            {t('wl.exportdialog.action.export')}
          </button>
        </>
      );
    case 'running':
      return (
        <button type="button" className={styles.secondary} onClick={job.cancel}>
          {t('wl.exportdialog.action.cancelExport')}
        </button>
      );
    case 'ready':
      return (
        <>
          <button type="button" className={styles.secondary} onClick={job.downloadAgain}>
            {t('wl.exportdialog.action.downloadAgain')}
          </button>
          {closeButton}
        </>
      );
    case 'cancelled':
    case 'failed':
      return (
        <>
          <button type="button" className={styles.secondary} onClick={job.reset}>
            {t('wl.exportdialog.action.retry')}
          </button>
          {closeButton}
        </>
      );
    case 'started':
      return closeButton;
  }
}
