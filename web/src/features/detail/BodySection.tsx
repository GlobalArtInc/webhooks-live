import { useMemo, useState } from 'react';
import { useTranslation } from 'react-i18next';
import type { Capture } from '../../api/types';
import { BodyViewer, type BodyLoader, type BodyViewMode } from '../body';
import styles from './CaptureDetail.module.css';
import { toBodyContent } from './view';

interface BodySectionProps {
  capture: Capture;
  pending: boolean;
  loadBody: BodyLoader;
  mode: BodyViewMode;
  wrap: boolean;
  onModeChange: (mode: BodyViewMode) => void;
  onWrapChange: (wrap: boolean) => void;
  onDownloadRaw: () => void;
}

export function BodySection({
  capture,
  pending,
  loadBody,
  mode,
  wrap,
  onModeChange,
  onWrapChange,
  onDownloadRaw,
}: BodySectionProps) {
  const { t } = useTranslation();
  const [search, setSearch] = useState('');
  const body = useMemo(() => toBodyContent(capture.body), [capture.body]);

  return (
    <div className={styles.stack}>
      {pending ? <p className={styles.note}>{t('wl.detail.body.largePending')}</p> : null}
      <BodyViewer
        bodyId={capture.id}
        body={body}
        loadBody={loadBody}
        contentType={capture.content_type ?? ''}
        size={capture.body_size}
        mode={mode}
        wrap={wrap}
        search={search}
        onModeChange={onModeChange}
        onWrapChange={onWrapChange}
        onSearchChange={setSearch}
        onDownloadRaw={onDownloadRaw}
      />
    </div>
  );
}
