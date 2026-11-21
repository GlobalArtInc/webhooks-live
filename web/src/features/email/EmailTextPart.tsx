import { useCallback } from 'react';
import { useTranslation } from 'react-i18next';
import type { Bytes } from '../body/types';
import { TEXT_LOAD_MAX_BYTES } from './constants';
import styles from './EmailDetail.module.css';
import { EscapedTextView } from './EscapedTextView';
import { safeFileName } from './parts';
import { fetchPartBlob, partHref } from './partsApi';
import type { EmailPart } from './types';

export interface EmailTextPartProps {
  endpointId: string;
  captureId: string;
  part: EmailPart | null;
}

export function EmailTextPart({ endpointId, captureId, part }: EmailTextPartProps) {
  const { t } = useTranslation();
  const index = part === null ? null : part.index;

  const load = useCallback(
    async (limit: number | null, signal: AbortSignal): Promise<Bytes> => {
      if (index === null) {
        return new Uint8Array(0);
      }
      const blob = await fetchPartBlob(endpointId, captureId, index, {
        view: 'text',
        maxBytes: TEXT_LOAD_MAX_BYTES,
        signal,
      });
      const bytes: Bytes = new Uint8Array(await blob.arrayBuffer());
      return limit === null ? bytes : bytes.subarray(0, limit);
    },
    [endpointId, captureId, index],
  );

  if (part === null) {
    return <p className={styles.note}>{t('wl.email.text.empty')}</p>;
  }
  const fileName = `${safeFileName(part.filename, `part-${part.index}`)}.txt`;
  const href = partHref(endpointId, captureId, part.index, { download: true });
  if (part.size > TEXT_LOAD_MAX_BYTES) {
    return (
      <div className={styles.notice}>
        <span>{t('wl.email.text.tooLarge')}</span>
        <a className={styles.action} href={href} download={fileName} rel="noopener">
          {t('wl.email.text.download')}
        </a>
      </div>
    );
  }
  return (
    <EscapedTextView
      viewId={`${captureId}:text:${part.index}`}
      size={part.size}
      label={t('wl.email.text.label')}
      downloadLabel={t('wl.email.text.download')}
      download={{ kind: 'link', href, fileName }}
      load={load}
    />
  );
}
