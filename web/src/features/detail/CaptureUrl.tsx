import { useId, useLayoutEffect, useRef, useState } from 'react';
import { useTranslation } from 'react-i18next';
import { CopyButton } from '../../components/CopyButton/CopyButton';
import { MethodBadge } from '../../components/MethodBadge/MethodBadge';
import { UrlText } from '../../components/UrlText/UrlText';
import styles from './CaptureDetail.module.css';

const OVERFLOW_TOLERANCE = 1;

export interface CaptureUrlProps {
  method: string;
  url: string;
}

export function CaptureUrl({ method, url }: CaptureUrlProps) {
  const { t } = useTranslation();
  const urlId = useId();
  const textRef = useRef<HTMLElement>(null);
  const [expanded, setExpanded] = useState(false);
  const [clamped, setClamped] = useState(false);

  useLayoutEffect(() => {
    const element = textRef.current;
    if (element === null || expanded) {
      return undefined;
    }
    const measure = () => {
      setClamped(element.scrollHeight > element.clientHeight + OVERFLOW_TOLERANCE);
    };
    measure();
    if (typeof ResizeObserver === 'undefined') {
      return undefined;
    }
    const observer = new ResizeObserver(measure);
    observer.observe(element);
    return () => {
      observer.disconnect();
    };
  }, [url, expanded]);

  return (
    <div className={styles.urlBlock}>
      <div className={styles.urlRow}>
        <MethodBadge method={method} />
        <code
          id={urlId}
          ref={textRef}
          className={expanded ? styles.url : `${styles.url} ${styles.urlClamped}`}
          data-expanded={expanded}
        >
          <UrlText url={url} />
        </code>
        <CopyButton value={url} label={t('wl.detail.summary.copyUrl')} idleText={t('wl.detail.summary.copyUrl')} />
      </div>
      {clamped || expanded ? (
        <button
          type="button"
          className={styles.urlToggle}
          aria-expanded={expanded}
          aria-controls={urlId}
          onClick={() => {
            setExpanded((current) => !current);
          }}
        >
          {expanded ? t('wl.detail.summary.showLessUrl') : t('wl.detail.summary.showFullUrl')}
        </button>
      ) : null}
    </div>
  );
}
