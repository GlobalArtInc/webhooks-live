import * as Dialog from '@radix-ui/react-dialog';
import * as Popover from '@radix-ui/react-popover';
import { useParams } from '@tanstack/react-router';
import { useState } from 'react';
import { useTranslation } from 'react-i18next';
import type { ViewportLayout } from '../endpoint/useViewport';
import { ENDPOINT_SEARCH_ROUTE_ID } from '../filters/useCaptureSearch';
import type { CaptureSearch, CaptureSearchPatch } from '../filters/captureSearch';
import { BuilderForm } from './BuilderForm';
import styles from './FilterBuilder.module.css';

export interface FilterBuilderProps {
  search: CaptureSearch;
  layout: ViewportLayout;
  onApply: (patch: CaptureSearchPatch) => void;
}

interface ScopedBuilderProps {
  search: CaptureSearch;
  onApply: (patch: CaptureSearchPatch) => void;
}

function ScopedBuilder({ search, onApply }: ScopedBuilderProps) {
  const { id } = useParams({ from: ENDPOINT_SEARCH_ROUTE_ID });
  return <BuilderForm endpointId={id} search={search} onApply={onApply} />;
}

export function FilterBuilder({ search, layout, onApply }: FilterBuilderProps) {
  const { t } = useTranslation();
  const [open, setOpen] = useState(false);
  const trigger = (
    <button type="button" className={styles.trigger}>
      {t('wl.filterbuilder.open')}
    </button>
  );
  const applyAndClose = (patch: CaptureSearchPatch) => {
    onApply(patch);
    setOpen(false);
  };

  if (layout === 'narrow') {
    return (
      <Dialog.Root open={open} onOpenChange={setOpen}>
        <Dialog.Trigger asChild>{trigger}</Dialog.Trigger>
        <Dialog.Portal>
          <Dialog.Overlay className={styles.overlay} />
          <Dialog.Content className={styles.sheet}>
            <Dialog.Title className={styles.title}>{t('wl.filterbuilder.title')}</Dialog.Title>
            <Dialog.Description className={styles.description}>{t('wl.filterbuilder.description')}</Dialog.Description>
            <ScopedBuilder search={search} onApply={applyAndClose} />
          </Dialog.Content>
        </Dialog.Portal>
      </Dialog.Root>
    );
  }

  return (
    <Popover.Root open={open} onOpenChange={setOpen}>
      <Popover.Trigger asChild>{trigger}</Popover.Trigger>
      <Popover.Portal>
        <Popover.Content
          className={styles.popover}
          aria-label={t('wl.filterbuilder.title')}
          align="start"
          sideOffset={6}
          collisionPadding={8}
        >
          <p className={styles.title}>{t('wl.filterbuilder.title')}</p>
          <p className={styles.description}>{t('wl.filterbuilder.description')}</p>
          <ScopedBuilder search={search} onApply={applyAndClose} />
        </Popover.Content>
      </Popover.Portal>
    </Popover.Root>
  );
}
