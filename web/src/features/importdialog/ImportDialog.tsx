import * as Dialog from '@radix-ui/react-dialog';
import { useEffect, useRef, useState, type ChangeEvent, type DragEvent, type RefObject } from 'react';
import { useTranslation } from 'react-i18next';
import { useReturnFocus } from '../../components/useReturnFocus';
import { inspectFile, type ChosenFile } from './importFile';
import { useImportJob, type ImportPhase } from './useImportJob';
import {
  DropZone,
  FailureStatus,
  FilePreview,
  OutcomeStatus,
  TargetChoice,
  TimesToggle,
  UploadStatus,
  type ImportTarget,
} from './ImportViews';
import { ModalContent } from '../../components/ModalContent/ModalContent';
import styles from './ImportDialog.module.css';

export interface ImportDialogProps {
  endpointId: string | null;
  returnFocusRef?: RefObject<HTMLElement | null>;
  onOpenChange: (open: boolean) => void;
  onOpenEndpoint: (endpointId: string) => void;
}

export function ImportDialog({ endpointId, returnFocusRef, onOpenChange, onOpenEndpoint }: ImportDialogProps) {
  const returnFocus = useReturnFocus(returnFocusRef);
  const { t, i18n } = useTranslation();
  const locale = i18n.language;
  const job = useImportJob();
  const [chosen, setChosen] = useState<ChosenFile | null>(null);
  const [target, setTarget] = useState<ImportTarget>(endpointId === null ? 'new' : 'existing');
  const [keepTimes, setKeepTimes] = useState(true);
  const [dragging, setDragging] = useState(false);
  const inputRef = useRef<HTMLInputElement>(null);
  const statusRef = useRef<HTMLDivElement>(null);
  const phase = job.phase;
  const configuring = phase.kind === 'idle';
  const ready = chosen !== null && chosen.inspection !== null && chosen.inspection.ok ? chosen.inspection : null;

  useEffect(() => {
    if (phase.kind !== 'idle') {
      statusRef.current?.focus();
    }
  }, [phase.kind]);

  function choose(file: File): void {
    setChosen({ file, inspection: null });
    void inspectFile(file).then((inspection) => {
      setChosen((current) => (current?.file === file ? { file, inspection } : current));
    });
  }

  function openPicker(): void {
    inputRef.current?.click();
  }

  function handleInput(event: ChangeEvent<HTMLInputElement>): void {
    const file = event.target.files?.[0] ?? null;
    event.target.value = '';
    if (file !== null) {
      choose(file);
    }
  }

  function handleDrop(event: DragEvent<HTMLDivElement>): void {
    event.preventDefault();
    setDragging(false);
    const file = event.dataTransfer.files[0] ?? null;
    if (file !== null) {
      choose(file);
    }
  }

  function startImport(): void {
    if (ready === null || chosen === null) {
      return;
    }
    job.start({
      endpointId: target === 'existing' ? endpointId : null,
      file: chosen.file,
      format: ready.preview.format,
      keepTimes,
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
        <ModalContent
          className={styles.content}
          onOpenAutoFocus={returnFocus.onOpenAutoFocus}
          onCloseAutoFocus={returnFocus.onCloseAutoFocus}
        >
          <Dialog.Title className={styles.title}>{t('wl.importdialog.title')}</Dialog.Title>
          <Dialog.Description className={styles.description}>{t('wl.importdialog.description')}</Dialog.Description>
          <input
            ref={inputRef}
            type="file"
            hidden
            accept=".json,.ndjson,application/json,application/x-ndjson"
            onChange={handleInput}
          />
          {configuring ? (
            <div className={styles.form}>
              {chosen === null ? (
                <DropZone
                  dragging={dragging}
                  onDraggingChange={setDragging}
                  onDrop={handleDrop}
                  onChoose={openPicker}
                />
              ) : (
                <FilePreview chosen={chosen} locale={locale} onChoose={openPicker} />
              )}
              {ready === null ? null : <TargetOrNote endpointId={endpointId} target={target} onChange={setTarget} />}
              {ready === null ? null : <TimesToggle checked={keepTimes} onChange={setKeepTimes} />}
            </div>
          ) : (
            <div ref={statusRef} tabIndex={-1} className={styles.status}>
              <StatusView phase={phase} locale={locale} onRetry={job.reset} />
            </div>
          )}
          <div className={styles.actions}>
            <ActionBar
              phase={phase}
              canImport={ready !== null}
              onImport={startImport}
              onCancelImport={job.cancel}
              onReset={job.reset}
              onOpenEndpoint={onOpenEndpoint}
              onClose={() => {
                handleOpenChange(false);
              }}
            />
          </div>
        </ModalContent>
      </Dialog.Portal>
    </Dialog.Root>
  );
}

interface TargetOrNoteProps {
  endpointId: string | null;
  target: ImportTarget;
  onChange: (target: ImportTarget) => void;
}

function TargetOrNote({ endpointId, target, onChange }: TargetOrNoteProps) {
  const { t } = useTranslation();
  if (endpointId === null) {
    return <p className={styles.note}>{t('wl.importdialog.target.newNote')}</p>;
  }
  return <TargetChoice value={target} onChange={onChange} />;
}

interface StatusViewProps {
  phase: ImportPhase;
  locale: string;
  onRetry: () => void;
}

function StatusView({ phase, locale, onRetry }: StatusViewProps) {
  const { t } = useTranslation();
  switch (phase.kind) {
    case 'idle':
      return null;
    case 'uploading':
      return <UploadStatus progress={phase.progress} locale={locale} />;
    case 'done':
      return <OutcomeStatus outcome={phase.outcome} created={phase.created} />;
    case 'cancelled':
      return (
        <p role="status" className={styles.note}>
          {t('wl.importdialog.result.cancelled')}
        </p>
      );
    case 'failed':
      return <FailureStatus failure={phase} onRetry={onRetry} />;
  }
}

interface ActionBarProps {
  phase: ImportPhase;
  canImport: boolean;
  onImport: () => void;
  onCancelImport: () => void;
  onReset: () => void;
  onOpenEndpoint: (endpointId: string) => void;
  onClose: () => void;
}

function ActionBar({ phase, canImport, onImport, onCancelImport, onReset, onOpenEndpoint, onClose }: ActionBarProps) {
  const { t } = useTranslation();
  const closeButton = (
    <button type="button" className={styles.secondary} onClick={onClose}>
      {t('wl.importdialog.action.close')}
    </button>
  );
  switch (phase.kind) {
    case 'idle':
      return (
        <>
          {closeButton}
          <button type="button" className={styles.primary} disabled={!canImport} onClick={onImport}>
            {t('wl.importdialog.action.import')}
          </button>
        </>
      );
    case 'uploading':
      return (
        <button type="button" className={styles.secondary} onClick={onCancelImport}>
          {t('wl.importdialog.action.cancelImport')}
        </button>
      );
    case 'done': {
      const createdId = phase.created ? phase.outcome.endpointId : null;
      return (
        <>
          {createdId === null ? null : (
            <button
              type="button"
              className={styles.primary}
              onClick={() => {
                onOpenEndpoint(createdId);
              }}
            >
              {t('wl.importdialog.action.open')}
            </button>
          )}
          {closeButton}
        </>
      );
    }
    case 'cancelled':
      return (
        <>
          <button type="button" className={styles.secondary} onClick={onReset}>
            {t('wl.importdialog.action.retry')}
          </button>
          {closeButton}
        </>
      );
    case 'failed':
      return closeButton;
  }
}
