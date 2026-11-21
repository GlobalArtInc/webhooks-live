import { useMemo, useState } from 'react';
import { useTranslation } from 'react-i18next';
import { formatByteSize } from '../../components/format/format';
import { decodeText } from '../body/bytes';
import { CodeViewer } from '../body/CodeViewer';
import type { Bytes } from '../body/types';
import { useAsyncValue } from '../body/useAsyncValue';
import { TEXT_LARGE_BYTES, TEXT_LOAD_MAX_BYTES, TEXT_PREVIEW_BYTES } from './constants';
import styles from './EmailDetail.module.css';
import { escapeMultiline } from './inert';

export interface LinkDownload {
  kind: 'link';
  href: string;
  fileName: string;
}

export interface ActionDownload {
  kind: 'action';
  run: () => void;
}

export type TextLoader = (limit: number | null, signal: AbortSignal) => Promise<Bytes>;

export type DownloadAction = LinkDownload | ActionDownload;

export interface EscapedTextViewProps {
  viewId: string;
  size: number;
  label: string;
  downloadLabel: string;
  download: DownloadAction;
  load: TextLoader;
  initial?: Bytes | null;
}

interface LoadKey {
  attempt: number;
}

interface SourceInput {
  large: boolean;
  wantFull: boolean;
  initial: Bytes | null;
  initialEnough: boolean;
  initialComplete: boolean;
  prefix: Bytes | null;
  full: Bytes | null;
}

interface PickedSource {
  bytes: Bytes | null;
  full: boolean;
}

function pickSource(input: SourceInput): PickedSource {
  if (input.large && input.wantFull) {
    if (input.initialComplete) {
      return { bytes: input.initial, full: true };
    }
    if (input.full !== null) {
      return { bytes: input.full, full: true };
    }
  }
  return { bytes: input.initialEnough ? input.initial : input.prefix, full: false };
}

function EscapedTextViewContent({ size, label, downloadLabel, download, load, initial = null }: EscapedTextViewProps) {
  const { t, i18n } = useTranslation();
  const locale = i18n.language;
  const [wrap, setWrap] = useState(true);
  const [wantFull, setWantFull] = useState(false);
  const [attempt, setAttempt] = useState(0);

  const large = size > TEXT_LARGE_BYTES;
  const prefixNeeded = large ? TEXT_PREVIEW_BYTES : size;
  const canLoadFull = size <= TEXT_LOAD_MAX_BYTES;
  const initialEnough = initial !== null && initial.length >= prefixNeeded;
  const initialComplete = initial !== null && initial.length >= size;

  const prefixKey = useMemo<LoadKey | null>(() => (initialEnough ? null : { attempt }), [initialEnough, attempt]);
  const fullKey = useMemo<LoadKey | null>(
    () => (large && wantFull && canLoadFull && !initialComplete ? { attempt } : null),
    [large, wantFull, canLoadFull, initialComplete, attempt],
  );
  const prefixLoad = useAsyncValue(prefixKey, (signal) => load(large ? TEXT_PREVIEW_BYTES : null, signal));
  const fullLoad = useAsyncValue(fullKey, (signal) => load(null, signal));

  const picked = pickSource({
    large,
    wantFull: wantFull && canLoadFull,
    initial,
    initialEnough,
    initialComplete,
    prefix: prefixLoad.status === 'done' ? prefixLoad.value : null,
    full: fullLoad.status === 'done' ? fullLoad.value : null,
  });
  const showingFull = picked.full;
  const shown = useMemo<Bytes | null>(() => {
    if (picked.bytes === null) {
      return null;
    }
    return !picked.full && picked.bytes.length > prefixNeeded ? picked.bytes.subarray(0, prefixNeeded) : picked.bytes;
  }, [picked.bytes, picked.full, prefixNeeded]);

  const text = useMemo(
    () => (shown === null ? '' : escapeMultiline(decodeText(shown, shown.length < size))),
    [shown, size],
  );

  const failed = prefixLoad.status === 'failed' || fullLoad.status === 'failed';
  const loadingFull = fullKey !== null && fullLoad.status === 'pending';
  const gated = large && !showingFull;

  return (
    <div className={styles.viewer}>
      <div className={styles.viewerControls}>
        <button
          type="button"
          className={styles.action}
          aria-pressed={wrap}
          onClick={() => {
            setWrap(!wrap);
          }}
        >
          {t('wl.email.view.wrap')}
        </button>
        {download.kind === 'link' ? (
          <a className={styles.action} href={download.href} download={download.fileName} rel="noopener">
            {downloadLabel}
          </a>
        ) : (
          <button type="button" className={styles.action} onClick={download.run}>
            {downloadLabel}
          </button>
        )}
      </div>
      {gated && shown !== null ? (
        <div className={styles.notice}>
          <span>
            {t('wl.email.view.showingFirst', {
              shown: formatByteSize(shown.length, locale),
              total: formatByteSize(size, locale),
            })}
          </span>
          {canLoadFull ? (
            <button
              type="button"
              className={styles.action}
              onClick={() => {
                setWantFull(true);
              }}
            >
              {t('wl.email.view.loadFull', { size: formatByteSize(size, locale) })}
            </button>
          ) : (
            <span>{t('wl.email.view.tooLargeForFull')}</span>
          )}
        </div>
      ) : null}
      {loadingFull ? (
        <p className={styles.status} role="status">
          {t('wl.email.view.loadingFull')}
        </p>
      ) : null}
      {failed ? (
        <div className={styles.notice} role="alert">
          <span>{t('wl.email.view.failed')}</span>
          <button
            type="button"
            className={styles.action}
            onClick={() => {
              setAttempt(attempt + 1);
            }}
          >
            {t('wl.email.view.retry')}
          </button>
        </div>
      ) : null}
      {shown === null ? null : <CodeViewer text={text} language="plain" wrap={wrap} search="" label={label} />}
      {shown === null && !failed ? (
        <p className={styles.status} role="status">
          {t('wl.email.view.loading')}
        </p>
      ) : null}
    </div>
  );
}

export function EscapedTextView(props: EscapedTextViewProps) {
  return <EscapedTextViewContent key={props.viewId} {...props} />;
}
