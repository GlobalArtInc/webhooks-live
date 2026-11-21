import type { ReactNode } from 'react';
import { useTranslation } from 'react-i18next';
import type { StateKind } from './stateKind';
import styles from './StatePage.module.css';

export interface StateDetail {
  status?: number;
  identity?: string;
  providerName?: string | null;
  signInHref?: string;
}

export interface StatePageProps {
  kind: StateKind;
  detail?: StateDetail;
  onRetry?: () => void;
  onSignOut?: () => void;
  actions?: ReactNode;
}

export function StatePage({ kind, detail = {}, onRetry, onSignOut, actions }: StatePageProps) {
  const { t } = useTranslation();

  let title: string;
  let body: string | null = null;
  switch (kind) {
    case 'gone':
      title = t('wl.state.gone.title');
      body = t('wl.state.gone.body');
      break;
    case 'not-found':
      title = t('wl.state.notFound.title');
      body = t('wl.state.notFound.body');
      break;
    case 'not-shared':
      title = t('wl.state.notShared.title');
      body = t('wl.state.notShared.body');
      break;
    case 'error':
      title = t('wl.state.error.title');
      body = t('wl.state.error.body');
      break;
    case 'signin':
      title = t('wl.state.signIn.title');
      break;
  }

  const signInLabel =
    typeof detail.providerName === 'string'
      ? t('wl.state.signIn.button', { provider: detail.providerName })
      : t('wl.state.signIn.buttonFallback');

  return (
    <section className={styles.page}>
      <h1 className={styles.title}>{title}</h1>
      {body !== null && <p className={styles.body}>{body}</p>}
      {kind === 'error' && detail.status !== undefined && (
        <p className={styles.detail}>{t('wl.state.error.status', { status: detail.status })}</p>
      )}
      {kind === 'not-shared' && detail.identity !== undefined && (
        <p className={styles.detail}>{t('wl.state.notShared.identity', { identity: detail.identity })}</p>
      )}
      <div className={styles.actions}>
        {kind === 'gone' && (
          <a className={styles.primary} href="/">
            {t('wl.common.createEndpoint')}
          </a>
        )}
        {kind === 'not-found' && (
          <a className={styles.primary} href="/">
            {t('wl.state.notFound.home')}
          </a>
        )}
        {kind === 'not-shared' && onSignOut !== undefined && (
          <button type="button" className={styles.secondary} onClick={onSignOut}>
            {t('wl.state.notShared.signOut')}
          </button>
        )}
        {kind === 'error' && onRetry !== undefined && (
          <button type="button" className={styles.primary} onClick={onRetry}>
            {t('wl.state.error.retry')}
          </button>
        )}
        {kind === 'signin' && (
          <a className={styles.primary} href={detail.signInHref ?? '/auth/login'}>
            {signInLabel}
          </a>
        )}
        {actions}
      </div>
    </section>
  );
}
