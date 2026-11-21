import { useMemo, useRef, type ReactNode } from 'react';
import { useTranslation } from 'react-i18next';
import type { Capture, ResponseConfig } from '../../api/types';
import { CopyAsMenu } from '../../components/CopyAsMenu/CopyAsMenu';
import type { CopyAsKind } from '../../components/CopyAsMenu/prepareCopyAs';
import { ForwardResultPanel } from '../../components/ForwardResultPanel/ForwardResultPanel';
import { KeyValueTable } from '../../components/KeyValueTable/KeyValueTable';
import { SectionNav, type SectionNavItem } from '../../components/SectionNav/SectionNav';
import type { BodyLoader, BodyViewMode } from '../body';
import { EmailDetail, readEmailView } from '../email';
import { BodySection } from './BodySection';
import { DetailSection } from './DetailSection';
import { FormSection } from './FormSection';
import styles from './CaptureDetail.module.css';
import { SummarySection } from './SummarySection';
import type { CaptureDetailState, DetailSectionId, FormFileView } from './types';
import { useActiveSection } from './useActiveSection';
import { parseRawQuery, visibleSections } from './view';

const HEADER_PREVIEW_ROWS = 12;

export interface CaptureDetailProps {
  state: CaptureDetailState;
  capture: Capture | null;
  endpointResponse?: ResponseConfig;
  loadBody: BodyLoader;
  bodyMode: BodyViewMode;
  onBodyModeChange: (mode: BodyViewMode) => void;
  wrap: boolean;
  onWrapChange: (wrap: boolean) => void;
  collapsedSections: readonly DetailSectionId[];
  onCollapsedSectionsChange: (sections: readonly DetailSectionId[]) => void;
  onResend: () => void;
  onDownloadRaw: () => void;
  onDownloadFormFile: (file: FormFileView) => void;
  onCopied: (kind: CopyAsKind) => void;
}

interface ReadyDetailProps extends Omit<CaptureDetailProps, 'state' | 'capture'> {
  capture: Capture;
}

interface PlaceholderProps {
  role: 'status' | 'alert';
  message: string;
}

function Placeholder({ role, message }: PlaceholderProps) {
  return (
    <p role={role} className={styles.placeholder}>
      {message}
    </p>
  );
}

export function CaptureDetail(props: CaptureDetailProps) {
  const { t } = useTranslation();
  const { state, capture, ...rest } = props;
  if (state === 'loading') {
    return <Placeholder role="status" message={t('wl.detail.state.loading')} />;
  }
  if (state === 'deleted') {
    return <Placeholder role="status" message={t('wl.detail.state.deleted')} />;
  }
  if (state === 'error') {
    return <Placeholder role="alert" message={t('wl.detail.state.error')} />;
  }
  if (capture === null) {
    return null;
  }
  const email = readEmailView(capture);
  if (email !== null) {
    return <EmailDetail key={email.capture.id} view={email} loadBody={rest.loadBody} />;
  }
  return <ReadyDetail {...rest} capture={capture} />;
}

function ReadyDetail({
  capture,
  endpointResponse,
  loadBody,
  bodyMode,
  onBodyModeChange,
  wrap,
  onWrapChange,
  collapsedSections,
  onCollapsedSectionsChange,
  onResend,
  onDownloadRaw,
  onDownloadFormFile,
  onCopied,
}: ReadyDetailProps) {
  const { t, i18n } = useTranslation();
  const locale = i18n.language;
  const rootRef = useRef<HTMLDivElement>(null);
  const sectionIds = useMemo(() => visibleSections(capture), [capture]);
  const active = useActiveSection(rootRef, sectionIds);
  const pending = capture.body.truncated;

  const titles: Record<DetailSectionId, string> = {
    summary: t('wl.detail.section.summary'),
    forward: t('wl.detail.section.forward'),
    query: t('wl.detail.section.query'),
    form: t('wl.detail.section.form'),
    body: t('wl.detail.section.body'),
    headers: t('wl.detail.section.headers'),
  };
  const navItems: SectionNavItem[] = sectionIds.map((id) => ({ id, label: titles[id] }));

  const isCollapsed = (id: DetailSectionId): boolean => collapsedSections.includes(id);

  const toggle = (id: DetailSectionId): void => {
    onCollapsedSectionsChange(
      isCollapsed(id) ? collapsedSections.filter((item) => item !== id) : [...collapsedSections, id],
    );
  };

  const navigate = (id: string): void => {
    const target = sectionIds.find((candidate) => candidate === id);
    if (target === undefined) {
      return;
    }
    if (isCollapsed(target)) {
      onCollapsedSectionsChange(collapsedSections.filter((item) => item !== target));
    }
    const section = rootRef.current?.querySelector<HTMLElement>(`[data-section="${target}"]`);
    section?.scrollIntoView({ block: 'start' });
    section?.querySelector<HTMLElement>('h2')?.focus({ preventScroll: true });
  };

  const contentOf = (id: DetailSectionId): ReactNode => {
    switch (id) {
      case 'summary':
        return <SummarySection capture={capture} />;
      case 'forward':
        return <ForwardResultPanel results={capture.forwards} onResend={onResend} />;
      case 'query':
        return (
          <KeyValueTable label={titles.query} rows={capture.query} rawRows={parseRawQuery(capture.query_string)} />
        );
      case 'form':
        return capture.form === null ? null : (
          <FormSection form={capture.form} locale={locale} onDownloadFile={onDownloadFormFile} />
        );
      case 'body':
        return (
          <BodySection
            capture={capture}
            pending={pending}
            loadBody={loadBody}
            mode={bodyMode}
            wrap={wrap}
            onModeChange={onBodyModeChange}
            onWrapChange={onWrapChange}
            onDownloadRaw={onDownloadRaw}
          />
        );
      case 'headers':
        return (
          <KeyValueTable label={titles.headers} rows={capture.headers} collapseAfter={HEADER_PREVIEW_ROWS} copyable />
        );
    }
  };

  return (
    <div ref={rootRef} className={styles.root}>
      <div className={styles.toolbar}>
        <CopyAsMenu capture={capture} endpointResponse={endpointResponse} onCopied={onCopied} />
      </div>
      <SectionNav sections={navItems} activeId={active} onSelect={navigate} />
      {sectionIds.map((id) => (
        <DetailSection key={id} id={id} title={titles[id]} collapsed={isCollapsed(id)} onToggle={toggle}>
          {contentOf(id)}
        </DetailSection>
      ))}
    </div>
  );
}
