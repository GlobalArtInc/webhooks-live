import * as DropdownMenu from '@radix-ui/react-dropdown-menu';
import { useMemo, useState } from 'react';
import { useTranslation } from 'react-i18next';
import type { Capture, ResponseConfig } from '../../api/types';
import styles from './CopyAsMenu.module.css';
import {
  prepareCopyAs,
  unsendableHeaders,
  type CopyAsItemId,
  type CopyAsKind,
  type PreparedAction,
  type ReadyAction,
} from './prepareCopyAs';

export interface CopyAsMenuProps {
  capture: Capture;
  endpointResponse?: ResponseConfig;
  onCopied: (kind: CopyAsKind) => void;
}

interface MenuItem {
  id: CopyAsItemId;
  label: string;
}

export function CopyAsMenu({ capture, endpointResponse, onCopied }: CopyAsMenuProps) {
  const { t } = useTranslation();
  const [open, setOpen] = useState(false);
  const [copyFailed, setCopyFailed] = useState(false);
  const plan = useMemo(
    () => (open ? prepareCopyAs(capture, endpointResponse) : null),
    [open, capture, endpointResponse],
  );
  const skipped = useMemo(() => unsendableHeaders(capture), [capture]);

  const copyItems: MenuItem[] = [
    { id: 'curl', label: t('wl.export.copyAs.curl') },
    { id: 'har', label: t('wl.export.copyAs.har') },
    { id: 'json', label: t('wl.export.copyAs.json') },
    { id: 'url', label: t('wl.export.copyAs.url') },
  ];
  const downloadItems: MenuItem[] = [
    { id: 'downloadJson', label: t('wl.export.download.json') },
    { id: 'downloadRaw', label: t('wl.export.download.raw') },
    { id: 'downloadHar', label: t('wl.export.download.har') },
  ];

  const runAction = (action: ReadyAction): void => {
    setCopyFailed(false);
    void action.run().then((succeeded) => {
      if (!succeeded) {
        setCopyFailed(true);
        return;
      }
      if (action.copies !== null) {
        onCopied(action.copies);
      }
    });
  };

  const renderItem = (item: MenuItem, action: PreparedAction) => (
    <DropdownMenu.Item
      key={item.id}
      className={styles.item}
      disabled={!action.ok}
      onSelect={() => {
        if (action.ok) {
          runAction(action);
        }
      }}
    >
      <span>{item.label}</span>
      {action.ok ? null : (
        <span className={styles.reason}>{t('wl.export.unavailable', { reason: action.reason })}</span>
      )}
    </DropdownMenu.Item>
  );

  return (
    <div className={styles.root}>
      <DropdownMenu.Root open={open} onOpenChange={setOpen}>
        <DropdownMenu.Trigger asChild>
          <button type="button" className={styles.trigger}>
            {t('wl.export.menu.trigger')}
          </button>
        </DropdownMenu.Trigger>
        <DropdownMenu.Portal>
          <DropdownMenu.Content className={styles.content} align="end" sideOffset={4}>
            {plan === null ? null : (
              <>
                {copyItems.map((item) => renderItem(item, plan[item.id]))}
                <DropdownMenu.Separator className={styles.separator} />
                {downloadItems.map((item) => renderItem(item, plan[item.id]))}
              </>
            )}
          </DropdownMenu.Content>
        </DropdownMenu.Portal>
      </DropdownMenu.Root>
      {skipped.length > 0 ? (
        <p className={styles.warning}>
          {t('wl.export.skippedHeaders', {
            count: skipped.length,
            names: skipped.map((header) => header.name).join(', '),
          })}
        </p>
      ) : null}
      {copyFailed ? (
        <p role="status" className={styles.warning}>
          {t('wl.export.copyFailed')}
        </p>
      ) : null}
    </div>
  );
}
