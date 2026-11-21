import { useCallback, useMemo, useRef, useState, type ReactNode } from 'react';
import { useTranslation } from 'react-i18next';
import { fetchCaptureBody } from '../../api/client';
import type { Capture } from '../../api/types';
import { downloadBlob } from '../../components/CopyAsMenu/download';
import { SectionNav, type SectionNavItem } from '../../components/SectionNav/SectionNav';
import { toBytes } from '../body/bytes';
import type { BodyLoader, Bytes } from '../body/types';
import { toBodyContent } from '../detail/view';
import { AttachmentsList } from './AttachmentsList';
import styles from './EmailDetail.module.css';
import { EmailSection } from './EmailSection';
import { EmailSummary } from './EmailSummary';
import { EmailTextPart } from './EmailTextPart';
import { EscapedTextView } from './EscapedTextView';
import { HeadersTable } from './HeadersTable';
import { InertText } from './InertText';
import { listedParts, pickTextPart, safeFileName } from './parts';
import { INITIALLY_COLLAPSED, type EmailSectionId } from './sections';
import type { EmailView } from './types';

const RAW_FILE_EXTENSION = 'eml';
const RAW_FILE_FALLBACK = 'message';

export interface EmailDetailProps {
  view: EmailView;
  loadBody: BodyLoader;
}

interface RawSourceProps {
  capture: Capture;
  loadBody: BodyLoader;
  onDownload: () => void;
}

function inlineBody(capture: Capture): Bytes | null {
  if (capture.body.truncated) {
    return null;
  }
  try {
    return toBytes(toBodyContent(capture.body));
  } catch {
    return null;
  }
}

function RawSource({ capture, loadBody, onDownload }: RawSourceProps) {
  const { t } = useTranslation();
  const initial = useMemo(() => inlineBody(capture), [capture]);
  return (
    <EscapedTextView
      viewId={`${capture.id}:raw`}
      size={capture.body_size}
      label={t('wl.email.raw.label')}
      downloadLabel={t('wl.email.raw.download')}
      download={{ kind: 'action', run: onDownload }}
      load={(limit) => loadBody(limit)}
      initial={initial}
    />
  );
}

export function EmailDetail({ view, loadBody }: EmailDetailProps) {
  const { t } = useTranslation();
  const { capture, meta } = view;
  const rootRef = useRef<HTMLDivElement>(null);
  const [collapsed, setCollapsed] = useState<readonly EmailSectionId[]>(INITIALLY_COLLAPSED);
  const [downloadFailed, setDownloadFailed] = useState(false);

  const textPart = useMemo(() => pickTextPart(meta.parts), [meta.parts]);
  const attachments = useMemo(() => listedParts(meta.parts, textPart), [meta.parts, textPart]);
  const sectionIds = useMemo<EmailSectionId[]>(
    () =>
      attachments.length > 0
        ? ['summary', 'text', 'attachments', 'headers', 'raw']
        : ['summary', 'text', 'headers', 'raw'],
    [attachments.length],
  );

  const titles: Record<EmailSectionId, string> = {
    summary: t('wl.email.section.summary'),
    text: t('wl.email.section.text'),
    attachments: t('wl.email.section.attachments'),
    headers: t('wl.email.section.headers'),
    raw: t('wl.email.section.raw'),
  };
  const navItems: SectionNavItem[] = sectionIds.map((id) => ({ id, label: titles[id] }));

  const toggle = (id: EmailSectionId): void => {
    setCollapsed((current) => (current.includes(id) ? current.filter((item) => item !== id) : [...current, id]));
  };

  const navigate = (id: string): void => {
    const target = sectionIds.find((candidate) => candidate === id);
    if (target === undefined) {
      return;
    }
    setCollapsed((current) => current.filter((item) => item !== target));
    const section = rootRef.current?.querySelector<HTMLElement>(`[data-section="${target}"]`);
    section?.scrollIntoView({ block: 'start' });
    section?.querySelector<HTMLElement>('h2')?.focus({ preventScroll: true });
  };

  const downloadRaw = useCallback(() => {
    setDownloadFailed(false);
    void fetchCaptureBody(capture.endpoint_id, capture.id, { download: true }).then(
      (blob) => {
        downloadBlob(blob, `${safeFileName(capture.id, RAW_FILE_FALLBACK)}.${RAW_FILE_EXTENSION}`);
      },
      () => {
        setDownloadFailed(true);
      },
    );
  }, [capture.endpoint_id, capture.id]);

  const contentOf = (id: EmailSectionId): ReactNode => {
    switch (id) {
      case 'summary':
        return <EmailSummary view={view} />;
      case 'text':
        return <EmailTextPart endpointId={capture.endpoint_id} captureId={capture.id} part={textPart} />;
      case 'attachments':
        return <AttachmentsList endpointId={capture.endpoint_id} captureId={capture.id} parts={attachments} />;
      case 'headers':
        return <HeadersTable rows={capture.headers} />;
      case 'raw':
        return capture.body_size > 0 ? (
          <RawSource capture={capture} loadBody={loadBody} onDownload={downloadRaw} />
        ) : (
          <p className={styles.note}>{t('wl.email.none')}</p>
        );
    }
  };

  return (
    <div ref={rootRef} className={styles.root} aria-label={t('wl.email.label')} role="group">
      {meta.parse_error === null ? null : (
        <div className={styles.banner} role="status">
          <p>{t('wl.email.notice.parseError')}</p>
          {meta.parse_error.detail === null ? null : (
            <p className={styles.mono}>
              {t('wl.email.notice.parseErrorDetail')} <InertText value={meta.parse_error.detail} />
            </p>
          )}
        </div>
      )}
      {meta.parts_truncated ? (
        <div className={styles.banner} role="status">
          <p>{t('wl.email.notice.partsTruncated')}</p>
        </div>
      ) : null}
      {downloadFailed ? (
        <p role="alert" className={styles.note}>
          {t('wl.email.raw.downloadFailed')}
        </p>
      ) : null}
      <SectionNav sections={navItems} activeId={null} onSelect={navigate} />
      {sectionIds.map((id) => (
        <EmailSection key={id} id={id} title={titles[id]} collapsed={collapsed.includes(id)} onToggle={toggle}>
          {contentOf(id)}
        </EmailSection>
      ))}
    </div>
  );
}
