import { useId, useState } from 'react';
import { useTranslation } from 'react-i18next';
import type { Capture } from '../../api/types';
import { DiffSummary } from '../DiffSummary/DiffSummary';
import { KIB, LARGE_BODY_LIMIT, MIB } from '../../features/diff/constants';
import { DiffEditor } from '../../features/diff/DiffEditor';
import { compareCaptures } from '../../features/diff/summary';
import { DEFAULT_NORMALISE_OPTIONS, normaliseCapture } from '../../features/diff/normalize';
import { useBodyDigest } from '../../features/diff/useBodyDigest';
import type { DiffLayout, NormaliseOptions } from '../../features/diff/types';
import styles from './DiffView.module.css';

export interface DiffViewProps {
  a: Capture;
  b: Capture;
  hideVolatile: boolean;
  sortHeaders: boolean;
  layout: DiffLayout;
  onHideVolatileChange: (value: boolean) => void;
  onSortHeadersChange: (value: boolean) => void;
  onLayoutChange: (layout: DiffLayout) => void;
  onSwap: () => void;
}

const SIDE_A = 'A';
const SIDE_B = 'B';

export function DiffView({
  a,
  b,
  hideVolatile,
  sortHeaders,
  layout,
  onHideVolatileChange,
  onSortHeadersChange,
  onLayoutChange,
  onSwap,
}: DiffViewProps) {
  const { t, i18n } = useTranslation();
  const bodyId = useId();
  const headersId = useId();
  const [largeConfirmed, setLargeConfirmed] = useState(false);

  const largeBodies = a.body_size > LARGE_BODY_LIMIT || b.body_size > LARGE_BODY_LIMIT;
  const compareBodies = !largeBodies || largeConfirmed;
  const digestA = useBodyDigest(a.body, compareBodies && !a.body.truncated);
  const digestB = useBodyDigest(b.body, compareBodies && !b.body.truncated);

  const options: NormaliseOptions = { ...DEFAULT_NORMALISE_OPTIONS, hideVolatile, sortHeaders };
  const left = normaliseCapture(a, { ...options, binaryDigest: digestA });
  const right = normaliseCapture(b, { ...options, binaryDigest: digestB });
  const comparison = compareCaptures(a, b, options, compareBodies);

  const formatter = new Intl.NumberFormat(i18n.language, { maximumFractionDigits: 1 });
  const sizeText = (size: number): string => {
    if (size >= MIB) {
      return t('wl.diff.size.mib', { value: formatter.format(size / MIB) });
    }
    if (size >= KIB) {
      return t('wl.diff.size.kib', { value: formatter.format(size / KIB) });
    }
    return t('wl.diff.size.bytes', { value: formatter.format(size) });
  };
  const limitText = sizeText(LARGE_BODY_LIMIT);

  const timeFormat = new Intl.DateTimeFormat(i18n.language, { timeStyle: 'medium' });
  const headersPart = t('wl.diff.part.headers');
  const bodyPart = t('wl.diff.part.body');

  return (
    <div className={styles.view}>
      <div className={styles.toolbar} role="group" aria-label={t('wl.diff.toolbar.label')}>
        <button
          type="button"
          className={styles.toggle}
          aria-pressed={hideVolatile}
          onClick={() => onHideVolatileChange(!hideVolatile)}
        >
          {t('wl.diff.toolbar.hideVolatile')}
        </button>
        <button
          type="button"
          className={styles.toggle}
          aria-pressed={sortHeaders}
          onClick={() => onSortHeadersChange(!sortHeaders)}
        >
          {t('wl.diff.toolbar.sortHeaders')}
        </button>
        <div className={styles.layoutGroup} role="group" aria-label={t('wl.diff.toolbar.layout')}>
          <button
            type="button"
            className={styles.toggle}
            aria-pressed={layout === 'split'}
            onClick={() => onLayoutChange('split')}
          >
            {t('wl.diff.toolbar.split')}
          </button>
          <button
            type="button"
            className={styles.toggle}
            aria-pressed={layout === 'unified'}
            onClick={() => onLayoutChange('unified')}
          >
            {t('wl.diff.toolbar.unified')}
          </button>
        </div>
        <button type="button" className={styles.action} onClick={onSwap}>
          {t('wl.diff.toolbar.swap')}
        </button>
      </div>

      <DiffSummary
        left={left.summary}
        right={right.summary}
        leftTime={timeFormat.format(new Date(a.received_at))}
        rightTime={timeFormat.format(new Date(b.received_at))}
        comparison={comparison}
      />

      <section className={styles.section} aria-labelledby={headersId}>
        <h2 id={headersId} className={styles.heading}>
          {t('wl.diff.section.headers')}
        </h2>
        <DiffEditor
          layout={layout}
          original={left.headers}
          modified={right.headers}
          labelOriginal={t('wl.diff.region.side', { side: SIDE_A, part: headersPart })}
          labelModified={t('wl.diff.region.side', { side: SIDE_B, part: headersPart })}
          labelUnified={t('wl.diff.region.unified', { part: headersPart })}
        />
      </section>

      <section className={styles.section} aria-labelledby={bodyId}>
        <h2 id={bodyId} className={styles.heading}>
          {t('wl.diff.section.body')}
        </h2>
        {compareBodies ? (
          <>
            {a.body.truncated || b.body.truncated ? (
              <p className={styles.note}>{t('wl.diff.body.truncated', { limit: limitText })}</p>
            ) : null}
            {a.body.encoding === 'base64' || b.body.encoding === 'base64' ? (
              <p className={styles.note}>{t('wl.diff.body.binary')}</p>
            ) : null}
            <DiffEditor
              layout={layout}
              original={left.body}
              modified={right.body}
              labelOriginal={t('wl.diff.region.side', { side: SIDE_A, part: bodyPart })}
              labelModified={t('wl.diff.region.side', { side: SIDE_B, part: bodyPart })}
              labelUnified={t('wl.diff.region.unified', { part: bodyPart })}
            />
          </>
        ) : (
          <div className={styles.gate}>
            <p className={styles.note}>
              {t('wl.diff.large.notice', {
                sizeA: sizeText(a.body_size),
                sizeB: sizeText(b.body_size),
                limit: limitText,
              })}
            </p>
            <button type="button" className={styles.action} onClick={() => setLargeConfirmed(true)}>
              {t('wl.diff.large.compare')}
            </button>
          </div>
        )}
      </section>
    </div>
  );
}
