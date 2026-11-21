import { useTranslation } from 'react-i18next';
import { ConfirmDialog } from '../../../components/ConfirmDialog/ConfirmDialog';
import { stepTitle } from './labels';
import styles from './Tester.module.css';
import { describeFailure } from './summary';
import type { RealRunResult } from './types';

export interface RealRunPanelProps {
  endpointId: string;
  target: string;
  effects: readonly string[];
  stepTitles: Readonly<Record<string, string>> | undefined;
  dirty: boolean;
  disabled: boolean;
  open: boolean;
  pending: boolean;
  result: RealRunResult | null;
  error: unknown;
  onOpenChange: (open: boolean) => void;
  onConfirm: () => void;
}

export function RealRunPanel({
  endpointId,
  target,
  effects,
  stepTitles,
  dirty,
  disabled,
  open,
  pending,
  result,
  error,
  onOpenChange,
  onConfirm,
}: RealRunPanelProps) {
  const { t } = useTranslation();
  const listed =
    effects.length === 0
      ? t('wl.tester.real.noEffects')
      : effects.map((kind) => stepTitle(kind, stepTitles)).join(', ');
  const body = `${t('wl.tester.real.body', { target, effects: listed })}${dirty ? ` ${t('wl.tester.real.dirty')}` : ''}`;
  const failure = error === null || error === undefined ? null : describeFailure(error, t);
  const send = result?.send ?? null;
  const captureId = send?.capture_id ?? null;

  return (
    <section className={styles.section} aria-label={t('wl.tester.real.title')}>
      <div className={styles.sectionHead}>
        <button
          type="button"
          className={styles.button}
          disabled={disabled || pending}
          onClick={() => {
            onOpenChange(true);
          }}
        >
          {pending ? t('wl.tester.real.sending') : t('wl.tester.real.button')}
        </button>
      </div>
      <ConfirmDialog
        open={open}
        title={t('wl.tester.real.confirmTitle')}
        body={body}
        confirmLabel={t('wl.tester.real.confirm')}
        cancelLabel={t('wl.tester.real.cancel')}
        busy={pending}
        onConfirm={onConfirm}
        onOpenChange={onOpenChange}
      />
      {failure === null ? null : (
        <p role="alert" className={styles.error}>
          {t('wl.tester.real.failed', { message: failure.message })}
        </p>
      )}
      {send === null ? null : (
        <div className={styles.row}>
          <p className={styles.note} role="status">
            {send.error === null
              ? t('wl.tester.real.sent', { status: send.status ?? 0, ms: send.duration_ms })
              : t('wl.tester.real.sendError', { error: send.error })}
          </p>
          {captureId === null ? (
            <span className={styles.note}>{t('wl.tester.real.noCapture')}</span>
          ) : (
            <a
              className={styles.link}
              href={`/e/${encodeURIComponent(endpointId)}/c/${encodeURIComponent(captureId)}`}
              target="_blank"
              rel="noopener noreferrer"
            >
              {t('wl.tester.real.open')}
            </a>
          )}
        </div>
      )}
    </section>
  );
}
