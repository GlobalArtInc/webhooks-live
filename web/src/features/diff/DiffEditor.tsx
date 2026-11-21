import { useEffect, useRef, useState } from 'react';
import { useTranslation } from 'react-i18next';
import styles from './DiffEditor.module.css';
import type { DiffLayout, MergeEditorHandle } from './types';

interface DiffEditorProps {
  layout: DiffLayout;
  original: string;
  modified: string;
  labelOriginal: string;
  labelModified: string;
  labelUnified: string;
}

type EditorStatus = 'loading' | 'ready' | 'failed';

export function DiffEditor({
  layout,
  original,
  modified,
  labelOriginal,
  labelModified,
  labelUnified,
}: DiffEditorProps) {
  const { t } = useTranslation();
  const hostRef = useRef<HTMLDivElement>(null);
  const [status, setStatus] = useState<EditorStatus>('loading');

  useEffect(() => {
    const host = hostRef.current;
    if (host === null) {
      return undefined;
    }
    let cancelled = false;
    let handle: MergeEditorHandle | null = null;
    import('./mergeEditor').then(
      (module) => {
        if (cancelled) {
          return;
        }
        handle = module.mountMergeEditor(host, {
          layout,
          original,
          modified,
          labelOriginal,
          labelModified,
          labelUnified,
        });
        setStatus('ready');
      },
      () => {
        if (!cancelled) {
          setStatus('failed');
        }
      },
    );
    return () => {
      cancelled = true;
      handle?.destroy();
      handle = null;
    };
  }, [layout, original, modified, labelOriginal, labelModified, labelUnified]);

  return (
    <div className={styles.viewer}>
      <div ref={hostRef} className={styles.host} />
      {status === 'loading' ? (
        <p className={styles.notice} role="status">
          {t('wl.diff.viewer.loading')}
        </p>
      ) : null}
      {status === 'failed' ? (
        <p className={styles.notice} role="alert">
          {t('wl.diff.viewer.failed')}
        </p>
      ) : null}
    </div>
  );
}
