import * as Dialog from '@radix-ui/react-dialog';
import { Fragment, useId, useRef } from 'react';
import { useTranslation } from 'react-i18next';
import { displayKey, SHORTCUT_SCOPES, type ShortcutEntry, type ShortcutScope } from '../../features/shortcuts/registry';
import type { SourceKey } from '../../i18n/keys';
import { useReturnFocus } from '../useReturnFocus';
import { ModalContent } from '../ModalContent/ModalContent';
import styles from './ShortcutsDialog.module.css';

const SCOPE_LABELS: Readonly<Record<ShortcutScope, SourceKey>> = {
  global: 'wl.shortcuts.scope.global',
  endpoint: 'wl.shortcuts.scope.endpoint',
  capture: 'wl.shortcuts.scope.capture',
};

export interface ShortcutsDialogProps {
  open: boolean;
  shortcuts: readonly ShortcutEntry[];
  singleKey: boolean;
  onSingleKeyChange: (enabled: boolean) => void;
  onOpenChange: (open: boolean) => void;
}

export function ShortcutsDialog({ open, shortcuts, singleKey, onSingleKeyChange, onOpenChange }: ShortcutsDialogProps) {
  const { t } = useTranslation();
  const idPrefix = useId();
  const returnFocus = useReturnFocus();
  const switchLabelId = `${idPrefix}-switch`;
  const titleRef = useRef<HTMLHeadingElement>(null);

  return (
    <Dialog.Root open={open} onOpenChange={onOpenChange}>
      <Dialog.Portal>
        <Dialog.Overlay className={styles.overlay} />
        <ModalContent
          className={styles.content}
          onOpenAutoFocus={(event) => {
            returnFocus.onOpenAutoFocus();
            event.preventDefault();
            titleRef.current?.focus();
          }}
          onCloseAutoFocus={returnFocus.onCloseAutoFocus}
        >
          <Dialog.Title ref={titleRef} tabIndex={-1} className={styles.title}>
            {t('wl.shortcuts.title')}
          </Dialog.Title>
          <Dialog.Description className={styles.hint}>{t('wl.shortcuts.hint')}</Dialog.Description>
          <div className={styles.preference}>
            <span id={switchLabelId} className={styles.preferenceLabel}>
              {t('wl.shortcuts.singleKey.label')}
            </span>
            <button
              type="button"
              role="switch"
              aria-checked={singleKey}
              aria-labelledby={switchLabelId}
              className={styles.switch}
              onClick={() => {
                onSingleKeyChange(!singleKey);
              }}
            >
              {singleKey ? t('wl.shortcuts.singleKey.on') : t('wl.shortcuts.singleKey.off')}
            </button>
          </div>
          {SHORTCUT_SCOPES.map((scope) => {
            const entries = shortcuts.filter((entry) => entry.scope === scope);
            const headingId = `${idPrefix}-${scope}`;
            return (
              <section key={scope} aria-labelledby={headingId} className={styles.group}>
                <h3 id={headingId} className={styles.heading}>
                  {t(SCOPE_LABELS[scope])}
                </h3>
                <dl className={styles.list}>
                  {entries.map((entry) => (
                    <div key={entry.id} className={styles.row}>
                      <dt className={styles.keys}>
                        {entry.keys.map((key, index) => (
                          <Fragment key={key}>
                            {index === 0 ? null : <span className={styles.separator}> / </span>}
                            <kbd className={styles.key}>{displayKey(key)}</kbd>
                          </Fragment>
                        ))}
                      </dt>
                      <dd className={styles.action}>{t(entry.label)}</dd>
                    </div>
                  ))}
                </dl>
              </section>
            );
          })}
          <div className={styles.actions}>
            <Dialog.Close asChild>
              <button type="button" className={styles.close}>
                {t('wl.shortcuts.close')}
              </button>
            </Dialog.Close>
          </div>
        </ModalContent>
      </Dialog.Portal>
    </Dialog.Root>
  );
}
