import { useRef, type MouseEvent, type ReactNode } from 'react';
import { useTranslation } from 'react-i18next';
import { OfflineBanner } from '../../components/OfflineBanner/OfflineBanner';
import { ThemeControl } from '../../components/ThemeControl/ThemeControl';
import { LanguageControl } from '../../i18n/LanguageControl';
import { ErrorBoundary } from './ErrorBoundary';
import styles from './AppShell.module.css';

export interface AppShellProps {
  header?: ReactNode;
  rail?: ReactNode;
  resetKey: string;
  children: ReactNode;
}

export function AppShell({ header, rail, resetKey, children }: AppShellProps) {
  const { t } = useTranslation();
  const mainRef = useRef<HTMLElement>(null);

  function focusMain(event: MouseEvent<HTMLAnchorElement>) {
    event.preventDefault();
    mainRef.current?.focus();
  }

  return (
    <div className={styles.shell}>
      <header className={styles.strip}>
        <a className={styles.skip} href="#main" onClick={focusMain}>
          {t('wl.shell.skipToMain')}
        </a>
        <a className={styles.wordmark} href="/">
          webhooks<span className={styles.dot}>.</span>live
        </a>
        {header}
        <div className={styles.controls}>
          <LanguageControl />
          <ThemeControl />
        </div>
      </header>
      <OfflineBanner />
      <div className={styles.body}>
        {rail !== undefined && <aside className={styles.rail}>{rail}</aside>}
        <main id="main" ref={mainRef} tabIndex={-1} className={styles.main}>
          <ErrorBoundary resetKey={resetKey}>{children}</ErrorBoundary>
        </main>
      </div>
    </div>
  );
}
