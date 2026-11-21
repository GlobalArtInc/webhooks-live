import { useSearch } from '@tanstack/react-router';
import { useTranslation } from 'react-i18next';
import {
  ComposerWorkspace,
  emptyDraft,
  seedFromCapture,
  useReplaySource,
  type ComposerSeed,
} from '../../features/composer';
import styles from './ComposeScreen.module.css';

const BLANK_SEED: ComposerSeed = { draft: emptyDraft(), banner: null, sourceCaptureId: null };

export function ComposeScreen() {
  const { t } = useTranslation();
  const { endpoint, capture } = useSearch({ from: '/shell/send' });
  const replay = useReplaySource(endpoint, capture);
  const replaying = endpoint !== undefined && capture !== undefined;
  const loading = replaying && replay.isPending;
  const seed = replaying && replay.data !== undefined ? seedFromCapture(replay.data) : BLANK_SEED;

  return (
    <section className={styles.page}>
      <h1 className={styles.title}>{t('wl.composer.title')}</h1>
      {loading ? <p className={styles.note}>{t('wl.composer.loading')}</p> : null}
      {replaying && replay.isError ? (
        <p role="alert" className={styles.error}>
          {t('wl.composer.error.capture')}
        </p>
      ) : null}
      {loading ? null : (
        <ComposerWorkspace key={`${endpoint ?? ''}:${capture ?? ''}`} endpointId={endpoint ?? null} seed={seed} />
      )}
    </section>
  );
}
