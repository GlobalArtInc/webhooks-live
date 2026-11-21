import type { ReactNode } from 'react';
import { useTranslation } from 'react-i18next';
import { useOneTimeToken } from './oneTimeToken';
import { useConfirmation } from './useConfirmation';
import styles from './emailpages.module.css';

export interface LandingCardProps {
  title: string;
  children: ReactNode;
}

export function LandingCard({ title, children }: LandingCardProps) {
  return (
    <main className={styles.page}>
      <section className={styles.card}>
        <h1 className={styles.title}>{title}</h1>
        {children}
      </section>
    </main>
  );
}

export interface ConfirmationScreenProps<T> {
  title: string;
  intro: string;
  actionLabel: string;
  invalidText: string;
  failedText: string;
  run: (token: string) => Promise<T>;
  doneText: (result: T) => string;
}

export function ConfirmationScreen<T>({
  title,
  intro,
  actionLabel,
  invalidText,
  failedText,
  run,
  doneText,
}: ConfirmationScreenProps<T>) {
  const { t } = useTranslation();
  const token = useOneTimeToken();
  const { phase, result, submit } = useConfirmation(token, run);

  if (phase === 'done' && result !== null) {
    return (
      <LandingCard title={title}>
        <p role="status" className={styles.body}>
          {doneText(result)}
        </p>
      </LandingCard>
    );
  }

  if (phase === 'invalid') {
    return (
      <LandingCard title={title}>
        <p role="alert" className={styles.error}>
          {invalidText}
        </p>
      </LandingCard>
    );
  }

  return (
    <LandingCard title={title}>
      <p className={styles.body}>{intro}</p>
      {phase === 'failed' ? (
        <p role="alert" className={styles.error}>
          {failedText}
        </p>
      ) : null}
      <div>
        <button
          type="button"
          className={styles.primary}
          disabled={phase === 'submitting'}
          aria-busy={phase === 'submitting'}
          onClick={() => {
            void submit();
          }}
        >
          {phase === 'submitting' ? t('wl.emailpages.working') : actionLabel}
        </button>
      </div>
    </LandingCard>
  );
}
