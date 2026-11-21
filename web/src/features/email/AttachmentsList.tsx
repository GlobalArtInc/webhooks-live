import { useState } from 'react';
import { useTranslation } from 'react-i18next';
import { formatByteSize } from '../../components/format/format';
import { ImagePreview } from '../body/ImagePreview';
import { sniffImageType } from '../body/classify';
import type { Bytes, ImageType } from '../body/types';
import { useAsyncValue } from '../body/useAsyncValue';
import { PREVIEW_MAX_BYTES, PREVIEW_MAX_PIXELS } from './constants';
import styles from './EmailDetail.module.css';
import { pixelCount, readImageSize, type ImageSize } from './imageSize';
import { escapeInert } from './inert';
import { InertText } from './InertText';
import { canPreview, rasterTypeOf, safeFileName } from './parts';
import { fetchPartBlob, partHref } from './partsApi';
import type { EmailPart } from './types';

export interface AttachmentsListProps {
  endpointId: string;
  captureId: string;
  parts: readonly EmailPart[];
}

interface PartPreviewProps {
  endpointId: string;
  captureId: string;
  part: EmailPart;
  image: ImageType;
}

interface PreviewReady {
  kind: 'ready';
  bytes: Bytes;
}

interface PreviewOversized {
  kind: 'oversized';
  size: ImageSize;
}

type PreviewResult = PreviewReady | PreviewOversized;

async function loadPreview(
  endpointId: string,
  captureId: string,
  part: EmailPart,
  image: ImageType,
  signal: AbortSignal,
): Promise<PreviewResult> {
  const blob = await fetchPartBlob(endpointId, captureId, part.index, { maxBytes: PREVIEW_MAX_BYTES, signal });
  const bytes: Bytes = new Uint8Array(await blob.arrayBuffer());
  if (sniffImageType(bytes) !== image) {
    throw new Error('The part does not match its declared image type');
  }
  const size = readImageSize(bytes, image);
  if (size === null) {
    throw new Error('The image dimensions could not be read');
  }
  return pixelCount(size) > PREVIEW_MAX_PIXELS ? { kind: 'oversized', size } : { kind: 'ready', bytes };
}

function PartPreview({ endpointId, captureId, part, image }: PartPreviewProps) {
  const { t } = useTranslation();
  const [key] = useState<object>(() => ({}));
  const state = useAsyncValue(key, (signal) => loadPreview(endpointId, captureId, part, image, signal));
  if (state.status === 'failed') {
    return (
      <p className={styles.status} role="alert">
        {t('wl.email.attachments.previewFailed')}
      </p>
    );
  }
  if (state.status !== 'done') {
    return (
      <p className={styles.status} role="status">
        {t('wl.email.attachments.previewLoading')}
      </p>
    );
  }
  if (state.value.kind === 'oversized') {
    return (
      <p className={styles.status} role="alert">
        {t('wl.email.attachments.previewTooLarge', { width: state.value.size.width, height: state.value.size.height })}
      </p>
    );
  }
  return <ImagePreview bytes={state.value.bytes} image={image} />;
}

export function AttachmentsList({ endpointId, captureId, parts }: AttachmentsListProps) {
  const { t, i18n } = useTranslation();
  const locale = i18n.language;
  const [openIndex, setOpenIndex] = useState<number | null>(null);
  const openPart = parts.find((part) => part.index === openIndex);
  const openImage = openPart === undefined ? null : rasterTypeOf(openPart.content_type);

  return (
    <div className={styles.stack}>
      <div className={styles.scroller}>
        <table aria-label={t('wl.email.attachments.table')} className={styles.dataTable}>
          <thead>
            <tr>
              <th scope="col">{t('wl.email.attachments.name')}</th>
              <th scope="col">{t('wl.email.attachments.type')}</th>
              <th scope="col">{t('wl.email.attachments.size')}</th>
              <th scope="col">{t('wl.email.attachments.actions')}</th>
            </tr>
          </thead>
          <tbody>
            {parts.map((part) => {
              const shownName = part.filename ?? t('wl.email.attachments.unnamed', { index: part.index });
              const accessibleName = escapeInert(shownName);
              const previewable = canPreview(part, PREVIEW_MAX_BYTES);
              const open = openIndex === part.index;
              return (
                <tr key={part.index}>
                  <th scope="row" className={styles.nameCell}>
                    <InertText value={shownName} />
                    {part.disposition === 'inline' ? (
                      <span className={styles.badge}>{t('wl.email.attachments.inline')}</span>
                    ) : null}
                  </th>
                  <td className={styles.mono}>
                    <InertText value={part.content_type} />
                  </td>
                  <td>{formatByteSize(part.size, locale)}</td>
                  <td>
                    <span className={styles.actions}>
                      <a
                        className={styles.action}
                        href={partHref(endpointId, captureId, part.index, { download: true })}
                        download={safeFileName(part.filename, `part-${part.index}`)}
                        rel="noopener"
                        aria-label={t('wl.email.attachments.downloadNamed', { name: accessibleName })}
                      >
                        {t('wl.email.attachments.download')}
                      </a>
                      {previewable ? (
                        <button
                          type="button"
                          className={styles.action}
                          aria-pressed={open}
                          aria-label={
                            open
                              ? t('wl.email.attachments.hidePreviewNamed', { name: accessibleName })
                              : t('wl.email.attachments.previewNamed', { name: accessibleName })
                          }
                          onClick={() => {
                            setOpenIndex(open ? null : part.index);
                          }}
                        >
                          {open ? t('wl.email.attachments.hidePreview') : t('wl.email.attachments.preview')}
                        </button>
                      ) : null}
                    </span>
                  </td>
                </tr>
              );
            })}
          </tbody>
        </table>
      </div>
      {openPart !== undefined && openImage !== null ? (
        <PartPreview
          key={`${captureId}:${openPart.index}`}
          endpointId={endpointId}
          captureId={captureId}
          part={openPart}
          image={openImage}
        />
      ) : null}
    </div>
  );
}
