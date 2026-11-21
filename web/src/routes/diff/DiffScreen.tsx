import { useNavigate, useParams } from '@tanstack/react-router';
import { useState } from 'react';
import { useTranslation } from 'react-i18next';
import { useCapture } from '../../api/queries';
import { DiffView } from '../../components/DiffView/DiffView';
import type { DiffLayout } from '../../features/diff/types';
import { useNarrowViewport } from '../../features/diff/useNarrowViewport';
import styles from './DiffScreen.module.css';

export function DiffScreen() {
  const { t } = useTranslation();
  const navigate = useNavigate();
  const params = useParams({ from: '/shell/e/$id/diff/$a/$b' });
  const left = useCapture(params.id, params.a);
  const right = useCapture(params.id, params.b);
  const narrow = useNarrowViewport();
  const [layoutChoice, setLayoutChoice] = useState<DiffLayout | null>(null);
  const [hideVolatile, setHideVolatile] = useState(true);
  const [sortHeaders, setSortHeaders] = useState(false);

  if (left.error !== null) {
    throw left.error;
  }
  if (right.error !== null) {
    throw right.error;
  }

  const layout = layoutChoice ?? (narrow ? 'unified' : 'split');

  function swap() {
    void navigate({ to: '/e/$id/diff/$a/$b', params: { id: params.id, a: params.b, b: params.a } });
  }

  return (
    <section className={styles.page}>
      <h1 className={styles.title}>{t('wl.screen.diff')}</h1>
      {left.data === undefined || right.data === undefined ? (
        <p className={styles.status} role="status">
          {t('wl.diff.loading')}
        </p>
      ) : (
        <DiffView
          key={`${params.a}:${params.b}`}
          a={left.data}
          b={right.data}
          hideVolatile={hideVolatile}
          sortHeaders={sortHeaders}
          layout={layout}
          onHideVolatileChange={setHideVolatile}
          onSortHeadersChange={setSortHeaders}
          onLayoutChange={(next) => setLayoutChoice(next)}
          onSwap={swap}
        />
      )}
    </section>
  );
}
