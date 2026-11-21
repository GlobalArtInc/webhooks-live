import type { DragEvent } from 'react';
import { useTranslation } from 'react-i18next';
import { formatAbsoluteTime, formatByteSize } from '../../components/format/format';
import type { ChosenFile, ExportPreview, FileProblem } from './importFile';
import type { ImportFailed, ImportFailureReason, ImportOutcome } from './importRequest';
import { useCountdown } from './useCountdown';
import styles from './ImportDialog.module.css';

export type ImportTarget = 'existing' | 'new';

const MAX_SHOWN_PROBLEMS = 5;

interface DropZoneProps {
  dragging: boolean;
  onDraggingChange: (dragging: boolean) => void;
  onDrop: (event: DragEvent<HTMLDivElement>) => void;
  onChoose: () => void;
}

export function DropZone({ dragging, onDraggingChange, onDrop, onChoose }: DropZoneProps) {
  const { t } = useTranslation();
  return (
    <div
      className={dragging ? styles.dropActive : styles.drop}
      onDragOver={(event) => {
        event.preventDefault();
        onDraggingChange(true);
      }}
      onDragLeave={() => {
        onDraggingChange(false);
      }}
      onDrop={onDrop}
    >
      <p className={styles.dropText}>{t('wl.importdialog.drop.text')}</p>
      <button type="button" className={styles.secondary} onClick={onChoose}>
        {t('wl.importdialog.drop.choose')}
      </button>
      <p className={styles.note}>{t('wl.importdialog.drop.hint')}</p>
    </div>
  );
}

interface FilePreviewProps {
  chosen: ChosenFile;
  locale: string;
  onChoose: () => void;
}

export function FilePreview({ chosen, locale, onChoose }: FilePreviewProps) {
  const { t } = useTranslation();
  const inspection = chosen.inspection;
  return (
    <div className={styles.preview}>
      <p className={styles.fileName}>{chosen.file.name}</p>
      {inspection === null ? (
        <p role="status" className={styles.note}>
          {t('wl.importdialog.file.reading')}
        </p>
      ) : null}
      {inspection !== null && !inspection.ok ? <ProblemNotice problem={inspection.problem} /> : null}
      {inspection !== null && inspection.ok ? (
        <PreviewFacts preview={inspection.preview} size={chosen.file.size} locale={locale} />
      ) : null}
      <div>
        <button type="button" className={styles.secondary} onClick={onChoose}>
          {t('wl.importdialog.file.change')}
        </button>
      </div>
    </div>
  );
}

interface ProblemNoticeProps {
  problem: FileProblem;
}

function ProblemNotice({ problem }: ProblemNoticeProps) {
  const { t } = useTranslation();
  switch (problem) {
    case 'zip':
      return (
        <p role="alert" className={styles.problem}>
          {t('wl.importdialog.file.rejected.zip')}
        </p>
      );
    case 'extension':
      return (
        <p role="alert" className={styles.problem}>
          {t('wl.importdialog.file.rejected.extension')}
        </p>
      );
    case 'empty':
      return (
        <p role="alert" className={styles.problem}>
          {t('wl.importdialog.file.rejected.empty')}
        </p>
      );
    case 'tooLarge':
      return (
        <p role="alert" className={styles.problem}>
          {t('wl.importdialog.file.rejected.tooLarge')}
        </p>
      );
    case 'notExport':
      return (
        <p role="alert" className={styles.problem}>
          {t('wl.importdialog.file.rejected.notExport')}
        </p>
      );
    case 'unreadable':
      return (
        <p role="alert" className={styles.problem}>
          {t('wl.importdialog.file.rejected.unreadable')}
        </p>
      );
  }
}

interface PreviewFactsProps {
  preview: ExportPreview;
  size: number;
  locale: string;
}

function PreviewFacts({ preview, size, locale }: PreviewFactsProps) {
  const { t } = useTranslation();
  const unknown = t('wl.importdialog.file.unknown');
  return (
    <dl className={styles.facts}>
      <Fact label={t('wl.importdialog.file.size')} value={formatByteSize(size, locale)} />
      <Fact label={t('wl.importdialog.file.format')} value={preview.format === 'ndjson' ? 'NDJSON' : 'JSON'} />
      <Fact
        label={t('wl.importdialog.file.version')}
        value={preview.version === null ? unknown : String(preview.version)}
      />
      <Fact label={t('wl.importdialog.file.endpoint')} value={preview.endpointTitle ?? unknown} />
      <Fact
        label={t('wl.importdialog.file.exportedAt')}
        value={preview.exportedAt === null ? unknown : formatAbsoluteTime(preview.exportedAt, locale)}
      />
    </dl>
  );
}

interface FactProps {
  label: string;
  value: string;
}

function Fact({ label, value }: FactProps) {
  return (
    <div className={styles.fact}>
      <dt className={styles.factLabel}>{label}</dt>
      <dd className={styles.factValue}>{value}</dd>
    </div>
  );
}

interface TargetChoiceProps {
  value: ImportTarget;
  onChange: (target: ImportTarget) => void;
}

export function TargetChoice({ value, onChange }: TargetChoiceProps) {
  const { t } = useTranslation();
  return (
    <fieldset className={styles.group}>
      <legend className={styles.legend}>{t('wl.importdialog.target.legend')}</legend>
      <label className={styles.option}>
        <input
          type="radio"
          name="import-target"
          value="existing"
          checked={value === 'existing'}
          onChange={() => {
            onChange('existing');
          }}
        />
        <span>{t('wl.importdialog.target.existing')}</span>
      </label>
      <label className={styles.option}>
        <input
          type="radio"
          name="import-target"
          value="new"
          checked={value === 'new'}
          onChange={() => {
            onChange('new');
          }}
        />
        <span>{t('wl.importdialog.target.new')}</span>
      </label>
    </fieldset>
  );
}

interface TimesToggleProps {
  checked: boolean;
  onChange: (checked: boolean) => void;
}

export function TimesToggle({ checked, onChange }: TimesToggleProps) {
  const { t } = useTranslation();
  return (
    <label className={styles.option}>
      <input
        type="checkbox"
        checked={checked}
        onChange={(event) => {
          onChange(event.target.checked);
        }}
      />
      <span>{t('wl.importdialog.times.label')}</span>
      <span className={styles.hint}>{t('wl.importdialog.times.hint')}</span>
    </label>
  );
}

interface UploadStatusProps {
  progress: number;
  locale: string;
}

export function UploadStatus({ progress, locale }: UploadStatusProps) {
  const { t } = useTranslation();
  const percent = new Intl.NumberFormat(locale, { style: 'percent' }).format(progress);
  return (
    <div className={styles.upload}>
      <progress className={styles.progress} value={progress} max={1} aria-label={t('wl.importdialog.progress.label')} />
      <p className={styles.note}>
        {progress >= 1
          ? t('wl.importdialog.progress.processing')
          : t('wl.importdialog.progress.uploading', { percent })}
      </p>
    </div>
  );
}

interface OutcomeStatusProps {
  outcome: ImportOutcome;
  created: boolean;
}

export function OutcomeStatus({ outcome, created }: OutcomeStatusProps) {
  const { t } = useTranslation();
  return (
    <div role="status" className={styles.outcome}>
      {outcome.imported === null ? null : <p>{t('wl.importdialog.result.imported', { count: outcome.imported })}</p>}
      {outcome.imported === 0 ? <p>{t('wl.importdialog.result.none')}</p> : null}
      {outcome.skipped === null || outcome.skipped === 0 ? null : (
        <p>{t('wl.importdialog.result.skipped', { count: outcome.skipped })}</p>
      )}
      {outcome.reason === null ? null : (
        <p>
          {outcome.reason === 'capacity'
            ? t('wl.importdialog.result.reason.capacity')
            : t('wl.importdialog.result.reason.storage')}
        </p>
      )}
      {created ? <p>{t('wl.importdialog.result.newEndpoint')}</p> : null}
    </div>
  );
}

interface FailureStatusProps {
  failure: ImportFailed;
  onRetry: () => void;
}

export function FailureStatus({ failure, onRetry }: FailureStatusProps) {
  const { t } = useTranslation();
  const remaining = useCountdown(failure.retryAfter ?? 0);
  const shown = failure.problems.slice(0, MAX_SHOWN_PROBLEMS);
  const hidden = failure.problems.length - shown.length;
  return (
    <div className={styles.failure}>
      <p role="alert" className={styles.failed}>
        <FailureMessage reason={failure.reason} />
      </p>
      {failure.retryAfter !== null && remaining > 0 ? (
        <p className={styles.note}>{t('wl.importdialog.failed.retryIn', { seconds: remaining })}</p>
      ) : null}
      {shown.length === 0 ? null : (
        <>
          <p className={styles.note}>{t('wl.importdialog.failed.problems')}</p>
          <ul className={styles.problems}>
            {shown.map((problem, index) => (
              <li key={`${problem.field}-${index}`}>
                <code className={styles.position}>{problem.field}</code> {problem.message}
              </li>
            ))}
          </ul>
          {hidden > 0 ? <p className={styles.note}>{t('wl.importdialog.failed.more', { count: hidden })}</p> : null}
        </>
      )}
      <div>
        <button type="button" className={styles.secondary} disabled={remaining > 0} onClick={onRetry}>
          {t('wl.importdialog.action.retry')}
        </button>
      </div>
    </div>
  );
}

interface FailureMessageProps {
  reason: ImportFailureReason;
}

function FailureMessage({ reason }: FailureMessageProps) {
  const { t } = useTranslation();
  switch (reason) {
    case 'tooLarge':
      return <>{t('wl.importdialog.failed.tooLarge')}</>;
    case 'unsupported':
      return <>{t('wl.importdialog.failed.unsupported')}</>;
    case 'invalidJson':
      return <>{t('wl.importdialog.failed.invalidJson')}</>;
    case 'validation':
      return <>{t('wl.importdialog.failed.validation')}</>;
    case 'busy':
      return <>{t('wl.importdialog.failed.busy')}</>;
    case 'unauthenticated':
      return <>{t('wl.importdialog.failed.unauthenticated')}</>;
    case 'forbidden':
      return <>{t('wl.importdialog.failed.forbidden')}</>;
    case 'gone':
      return <>{t('wl.importdialog.failed.gone')}</>;
    case 'network':
      return <>{t('wl.importdialog.failed.network')}</>;
    case 'generic':
      return <>{t('wl.importdialog.failed.generic')}</>;
  }
}
