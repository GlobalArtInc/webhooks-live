import { useEffect, useId, useMemo, useRef, useState, type MouseEvent, type ReactNode } from 'react';
import { useTranslation } from 'react-i18next';
import { OfflineBanner } from '../../components/OfflineBanner/OfflineBanner';
import { ThemeControl } from '../../components/ThemeControl/ThemeControl';
import { ShortcutsButton } from '../../features/shortcuts';
import { LanguageControl } from '../../i18n/LanguageControl';
import { ErrorBoundary } from './ErrorBoundary';
import { RailContext, type RailState } from './railState';
import styles from './AppShell.module.css';

export interface AppShellProps {
  header?: ReactNode;
  rail?: ReactNode;
  resetKey: string;
  children: ReactNode;
}

function classNames(...names: (string | false | undefined)[]): string {
  return names.filter((name): name is string => typeof name === 'string').join(' ');
}

export function AppShell({ header, rail, resetKey, children }: AppShellProps) {
  const { t } = useTranslation();
  const mainRef = useRef<HTMLElement>(null);
  const menuRef = useRef<HTMLButtonElement>(null);
  const railRef = useRef<HTMLElement>(null);
  const railId = useId();
  const [collapsed, setCollapsed] = useState(false);
  const [drawerOpen, setDrawerOpen] = useState(false);

  const railState = useMemo<RailState>(
    () => ({
      collapsed,
      drawerOpen,
      toggleCollapsed: () => setCollapsed((current) => !current),
      toggleDrawer: () => setDrawerOpen((current) => !current),
      closeDrawer: () => setDrawerOpen(false),
    }),
    [collapsed, drawerOpen],
  );

  useEffect(() => {
    if (!drawerOpen) {
      return;
    }
    railRef.current?.querySelector<HTMLElement>('a, button')?.focus();
    function closeOnEscape(event: KeyboardEvent) {
      if (event.key === 'Escape') {
        setDrawerOpen(false);
        menuRef.current?.focus();
      }
    }
    window.addEventListener('keydown', closeOnEscape);
    return () => window.removeEventListener('keydown', closeOnEscape);
  }, [drawerOpen]);

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
        {rail !== undefined && (
          <button
            ref={menuRef}
            type="button"
            className={styles.menu}
            aria-expanded={drawerOpen}
            aria-controls={railId}
            onClick={railState.toggleDrawer}
          >
            {t('wl.home.rail.menu')}
          </button>
        )}
        <a className={styles.wordmark} href="/">
          webhooks<span className={styles.dot}>.</span>live
        </a>
        {header}
        <div className={styles.controls}>
          <ShortcutsButton />
          <LanguageControl />
          <ThemeControl />
        </div>
      </header>
      <OfflineBanner />
      <RailContext value={railState}>
        <div className={styles.body}>
          {rail !== undefined && (
            <aside
              id={railId}
              ref={railRef}
              className={classNames(styles.rail, collapsed && styles.railCollapsed, drawerOpen && styles.railOpen)}
            >
              {rail}
            </aside>
          )}
          <main id="main" ref={mainRef} tabIndex={-1} className={styles.main}>
            <ErrorBoundary resetKey={resetKey}>{children}</ErrorBoundary>
          </main>
        </div>
      </RailContext>
    </div>
  );
}
