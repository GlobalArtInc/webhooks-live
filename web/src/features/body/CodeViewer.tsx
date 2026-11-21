import { useEffect, useEffectEvent, useImperativeHandle, useRef, useState } from 'react';
import styles from './CodeViewer.module.css';
import type { CodeViewerProps, EditorConfig, EditorHandle } from './types';

interface LoadFailure {
  message: string;
}

type EditorStatus = 'loading' | 'ready' | LoadFailure;

export function CodeViewer({ text, language, wrap, search, label, ref }: CodeViewerProps) {
  const hostRef = useRef<HTMLDivElement>(null);
  const editorRef = useRef<EditorHandle | null>(null);
  const [status, setStatus] = useState<EditorStatus>('loading');

  const currentConfig = useEffectEvent((): EditorConfig => ({ text, language, wrap, search, label }));

  useImperativeHandle(
    ref,
    () => ({
      findNext: () => editorRef.current?.findNext(),
      findPrevious: () => editorRef.current?.findPrevious(),
    }),
    [],
  );

  useEffect(() => {
    const host = hostRef.current;
    if (host === null) {
      return undefined;
    }
    let cancelled = false;
    import('./editor').then(
      (module) => {
        if (cancelled) {
          return;
        }
        editorRef.current = module.createEditor(host, currentConfig());
        setStatus('ready');
      },
      (error: unknown) => {
        if (!cancelled) {
          setStatus({ message: error instanceof Error ? error.message : 'Unknown error' });
        }
      },
    );
    return () => {
      cancelled = true;
      editorRef.current?.destroy();
      editorRef.current = null;
    };
  }, []);

  useEffect(() => {
    if (status === 'ready') {
      editorRef.current?.update({ text, language, wrap, search, label });
    }
  }, [status, text, language, wrap, search, label]);

  return (
    <div className={styles.viewer}>
      <div ref={hostRef} className={styles.host} />
      {status === 'loading' ? (
        <p className={styles.notice} role="status">
          Loading viewer
        </p>
      ) : null}
      {typeof status === 'object' ? (
        <p className={styles.notice} role="alert">
          Could not load the code viewer: {status.message}
        </p>
      ) : null}
    </div>
  );
}
