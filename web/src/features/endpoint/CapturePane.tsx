import { useEffect, useRef, useState } from 'react';
import { useTranslation } from 'react-i18next';
import { ApiError } from '../../api/client';
import { useCapture } from '../../api/queries';
import { prepareCopyAs } from '../../components/CopyAsMenu/prepareCopyAs';
import { ListPager, type ListPagerProps } from '../../components/ListPager/ListPager';
import { saveBodyView, toggleBodyMode, toggleBodyWrap, useBodyViewPreference } from '../../prefs/layout';
import { CaptureDetailPanel, type DetailSectionId } from '../detail';
import { ResendDialog } from '../forwarding/ResendDialog';
import { compareLabelOf } from '../list/compare';
import { useShortcutHandlers } from '../shortcuts';
import styles from './CapturePane.module.css';

const NOT_FOUND = 404;
const SHORT_ID_LENGTH = 8;
const ignore = () => undefined;

export interface CapturePaneProps {
  endpointId: string;
  captureId: string;
  markedId: string | null;
  narrow: boolean;
  pager: ListPagerProps | null;
  onBack?: () => void;
  onClose?: () => void;
  onCompare?: () => void;
}

function CaptureMissing() {
  const { t } = useTranslation();
  return (
    <div className={styles.missing}>
      <p className={styles.missingTitle}>{t('wl.endpoint.detail.notFound.title')}</p>
      <p className={styles.missingBody}>{t('wl.endpoint.detail.notFound.body')}</p>
    </div>
  );
}

export function CapturePane({
  endpointId,
  captureId,
  markedId,
  narrow,
  pager,
  onBack,
  onClose,
  onCompare,
}: CapturePaneProps) {
  const { t } = useTranslation();
  const headingRef = useRef<HTMLHeadingElement>(null);
  const query = useCapture(endpointId, captureId);
  const capture = query.data;
  const { mode: bodyMode, wrap } = useBodyViewPreference();
  const [collapsed, setCollapsed] = useState<readonly DetailSectionId[]>([]);
  const [resendOpen, setResendOpen] = useState(false);
  const missing = query.error instanceof ApiError && query.error.status === NOT_FOUND;
  const compareLabel = compareLabelOf(markedId, captureId);

  useEffect(() => {
    if (narrow) {
      headingRef.current?.focus({ preventScroll: true });
    }
  }, [captureId, narrow]);

  useShortcutHandlers({
    copyCurl: () => {
      if (capture === undefined) {
        return;
      }
      const curl = prepareCopyAs(capture, undefined).curl;
      if (curl.ok) {
        void curl.run();
      }
    },
    prettyRaw: () => {
      toggleBodyMode();
    },
    wrap: () => {
      toggleBodyWrap();
    },
  });

  const compareText =
    compareLabel === 'mark'
      ? t('wl.endpoint.compare.mark')
      : compareLabel === 'unmark'
        ? t('wl.endpoint.compare.unmark')
        : t('wl.endpoint.compare.withMarked');

  return (
    <section className={styles.pane} aria-label={t('wl.endpoint.detail.label')}>
      <header className={styles.header}>
        {onBack === undefined ? null : (
          <button type="button" className={styles.button} onClick={onBack}>
            {t('wl.endpoint.detail.back')}
          </button>
        )}
        <h2 ref={headingRef} tabIndex={-1} className={styles.heading}>
          {t('wl.endpoint.detail.heading', { id: captureId.slice(-SHORT_ID_LENGTH) })}
        </h2>
        {onCompare === undefined ? null : (
          <button type="button" className={styles.button} onClick={onCompare}>
            {compareText}
          </button>
        )}
        {onClose === undefined ? null : (
          <button type="button" className={styles.button} onClick={onClose}>
            {t('wl.endpoint.detail.close')}
          </button>
        )}
      </header>
      <div className={styles.body}>
        {missing ? (
          <CaptureMissing />
        ) : (
          <CaptureDetailPanel
            endpointId={endpointId}
            captureId={captureId}
            bodyMode={bodyMode}
            onBodyModeChange={(mode) => saveBodyView({ mode })}
            wrap={wrap}
            onWrapChange={(next) => saveBodyView({ wrap: next })}
            collapsedSections={collapsed}
            onCollapsedSectionsChange={setCollapsed}
            onResend={() => {
              setResendOpen(true);
            }}
            onDownloadFormFile={ignore}
            onCopied={ignore}
          />
        )}
        {missing ? null : (
          <ResendDialog endpointId={endpointId} captureId={captureId} open={resendOpen} onOpenChange={setResendOpen} />
        )}
      </div>
      {pager === null ? null : <ListPager {...pager} />}
    </section>
  );
}
