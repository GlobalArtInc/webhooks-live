import * as DropdownMenu from '@radix-ui/react-dropdown-menu';
import { useNavigate } from '@tanstack/react-router';
import { lazy, Suspense, useState } from 'react';
import { useTranslation } from 'react-i18next';
import { offersImport } from '../../api/capabilities';
import { useInstance } from '../../api/queries';
import { copyText } from '../CopyButton/clipboard';
import styles from './EndpointHeader.module.css';

const ImportDialog = lazy(() =>
  import('../../features/importdialog/ImportDialog').then((module) => ({ default: module.ImportDialog })),
);

export interface HeaderMenuProps {
  endpointId: string;
  curl: string;
  disableDeleteAll: boolean;
  onDeleteAll: () => void;
}

export function HeaderMenu({ endpointId, curl, disableDeleteAll, onDeleteAll }: HeaderMenuProps) {
  const { t } = useTranslation();
  const navigate = useNavigate();
  const [importOpen, setImportOpen] = useState(false);
  const importOffered = offersImport(useInstance().data);
  return (
    <>
      <DropdownMenu.Root>
        <DropdownMenu.Trigger asChild>
          <button type="button" className={styles.button}>
            {t('wl.endpoint.header.actions')}
          </button>
        </DropdownMenu.Trigger>
        <DropdownMenu.Portal>
          <DropdownMenu.Content className={styles.menu} align="end" sideOffset={4}>
            <DropdownMenu.Item
              className={styles.menuItem}
              onSelect={() => {
                void copyText(curl);
              }}
            >
              {t('wl.endpoint.header.copyCurl')}
            </DropdownMenu.Item>
            <DropdownMenu.Item
              className={styles.menuItem}
              onSelect={() => {
                void navigate({ to: '/e/$id/settings', params: { id: endpointId } });
              }}
            >
              {t('wl.endpoint.header.settings')}
            </DropdownMenu.Item>
            <DropdownMenu.Item
              className={styles.menuItem}
              onSelect={() => {
                void navigate({ to: '/e/$id/forwarding', params: { id: endpointId } });
              }}
            >
              {t('wl.endpoint.header.forwarding')}
            </DropdownMenu.Item>
            {importOffered ? (
              <DropdownMenu.Item
                className={styles.menuItem}
                onSelect={() => {
                  setImportOpen(true);
                }}
              >
                {t('wl.importdialog.open')}
              </DropdownMenu.Item>
            ) : null}
            <DropdownMenu.Item className={styles.menuItem} disabled={disableDeleteAll} onSelect={onDeleteAll}>
              {t('wl.endpoint.header.deleteAll')}
            </DropdownMenu.Item>
          </DropdownMenu.Content>
        </DropdownMenu.Portal>
      </DropdownMenu.Root>
      {importOpen ? (
        <Suspense fallback={null}>
          <ImportDialog
            endpointId={endpointId}
            onOpenChange={setImportOpen}
            onOpenEndpoint={(openId) => {
              setImportOpen(false);
              void navigate({ to: '/e/$id', params: { id: openId } });
            }}
          />
        </Suspense>
      ) : null}
    </>
  );
}
