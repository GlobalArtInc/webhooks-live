import * as Dialog from '@radix-ui/react-dialog';
import { useId, useMemo, useRef, useState, type MouseEvent, type ReactNode } from 'react';
import { useTranslation } from 'react-i18next';
import { OfflineBanner } from '../../components/OfflineBanner/OfflineBanner';
import { ThemeControl } from '../../components/ThemeControl/ThemeControl';
import { NARROW_QUERY } from '../../features/endpoint/constants';
import { useMediaQuery } from '../../features/endpoint/useViewport';
import { ShortcutsButton } from '../../features/shortcuts';
import { LanguageControl } from '../../i18n/LanguageControl';
import { ErrorBoundary } from './ErrorBoundary';
import { RailContext, type RailState } from './railState';
import { ModalContent } from '../../components/ModalContent/ModalContent';
import styles from './AppShell.module.css';

const DRAWER_QUERY = '(max-width: 1199px)';
const SKIP_TARGET_SELECTOR = '[data-skip-target]';

export interface AppShellProps {
  header?: ReactNode;
  rail?: ReactNode;
  resetKey: string;
  collapseRail?: boolean;
  fill?: boolean;
  children: ReactNode;
}

function classNames(...names: (string | false | undefined)[]): string {
  return names.filter((name): name is string => typeof name === 'string').join(' ');
}

export function AppShell({ header, rail, resetKey, collapseRail = false, fill = false, children }: AppShellProps) {
  const { t } = useTranslation();
  const mainRef = useRef<HTMLElement>(null);
  const railId = useId();
  const drawerMode = useMediaQuery(DRAWER_QUERY);
  const narrow = useMediaQuery(NARROW_QUERY);
  const [collapsed, setCollapsed] = useState(collapseRail);
  const [appliedCollapseRail, setAppliedCollapseRail] = useState(collapseRail);
  const [drawerOpen, setDrawerOpen] = useState(false);
  const hasDrawer = drawerMode && (rail !== undefined || narrow);

  if (appliedCollapseRail !== collapseRail) {
    setAppliedCollapseRail(collapseRail);
    setCollapsed(collapseRail);
  }

  if (!hasDrawer && drawerOpen) {
    setDrawerOpen(false);
  }

  const railState = useMemo<RailState>(
    () => ({
      collapsed: drawerMode ? false : collapsed,
      drawerOpen,
      toggleCollapsed: () => setCollapsed((current) => !current),
      toggleDrawer: () => setDrawerOpen((current) => !current),
      closeDrawer: () => setDrawerOpen(false),
    }),
    [collapsed, drawerMode, drawerOpen],
  );

  function focusMain(event: MouseEvent<HTMLAnchorElement>) {
    event.preventDefault();
    const target = mainRef.current?.querySelector<HTMLElement>(SKIP_TARGET_SELECTOR) ?? mainRef.current;
    target?.focus();
  }

  const controls = (
    <div className={styles.controls}>
      <ShortcutsButton />
      <LanguageControl />
      <ThemeControl />
    </div>
  );

  return (
    <div className={classNames(styles.shell, fill && styles.shellFill)}>
      <header className={styles.strip}>
        <a className={styles.skip} href="#main" onClick={focusMain}>
          {t('wl.shell.skipToMain')}
        </a>
        {hasDrawer && (
          <Dialog.Root open={drawerOpen} onOpenChange={setDrawerOpen}>
            <Dialog.Trigger asChild>
              <button type="button" className={styles.menu}>
                {t('wl.home.rail.menu')}
              </button>
            </Dialog.Trigger>
            <Dialog.Portal>
              <Dialog.Overlay className={styles.overlay} />
              <ModalContent className={styles.drawer} aria-describedby={undefined}>
                <Dialog.Title className={styles.srOnly}>{t('wl.shell.menuTitle')}</Dialog.Title>
                <Dialog.Close className={styles.drawerClose}>{t('wl.shell.menuClose')}</Dialog.Close>
                <RailContext value={railState}>{rail}</RailContext>
                {narrow && (
                  <div className={styles.extras}>
                    {header}
                    {controls}
                  </div>
                )}
              </ModalContent>
            </Dialog.Portal>
          </Dialog.Root>
        )}
        <a className={styles.wordmark} href="/">
          webhooks<span className={styles.dot}>.</span>live
        </a>
        {narrow ? null : header}
        {narrow ? null : controls}
      </header>
      <OfflineBanner />
      <RailContext value={railState}>
        <div className={styles.body}>
          {rail !== undefined && !drawerMode && (
            <aside
              id={railId}
              className={classNames(styles.rail, railState.collapsed && styles.railCollapsed)}
              data-collapsed={railState.collapsed ? 'true' : undefined}
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
