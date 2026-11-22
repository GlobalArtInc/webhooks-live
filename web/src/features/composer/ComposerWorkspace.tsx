import { useState } from 'react';
import { useTranslation } from 'react-i18next';
import { canSignIn, offersSendHistory } from '../../api/capabilities';
import { useInstance } from '../../api/queries';
import type { SentRequest } from '../../api/types';
import { ComposerForm } from './ComposerForm';
import { seedFromSent, type ComposerSeed } from './draft';
import { HistoryList } from './HistoryList';
import { historyFailureOf } from './sendErrors';
import { useDeleteSentRequest, useSentHistory } from './useComposer';
import styles from './ComposerWorkspace.module.css';

export interface ComposerWorkspaceProps {
  endpointId: string | null;
  seed: ComposerSeed;
}

interface ComposerSession extends ComposerSeed {
  key: number;
}

export function ComposerWorkspace({ endpointId, seed }: ComposerWorkspaceProps) {
  const { t } = useTranslation();
  const [session, setSession] = useState<ComposerSession>(() => ({ ...seed, key: 0 }));
  const instance = useInstance().data;
  const historyOffered = offersSendHistory(instance, endpointId);
  const history = useSentHistory(endpointId, historyOffered);
  const remove = useDeleteSentRequest();

  function replay(entry: SentRequest): void {
    const next = seedFromSent(entry);
    setSession((current) => ({ ...next, key: current.key + 1 }));
  }

  function clearBanner(): void {
    setSession((current) => ({ ...current, banner: null }));
  }

  return (
    <div className={styles.workspace}>
      {session.banner === null ? null : (
        <div className={styles.banner}>
          <p className={styles.bannerText}>
            {t('wl.composer.replay.banner', { method: session.banner.method, target: session.banner.target })}
          </p>
          <button type="button" className={styles.clear} onClick={clearBanner}>
            {t('wl.composer.replay.clear')}
          </button>
        </div>
      )}
      <ComposerForm
        key={session.key}
        endpointId={endpointId}
        sourceCaptureId={session.sourceCaptureId}
        initialDraft={session.draft}
      />
      <HistoryList
        entries={history.data?.items}
        loading={instance === undefined || history.isLoading}
        offered={historyOffered}
        signInAvailable={canSignIn(instance)}
        failure={history.isError ? historyFailureOf(history.error) : null}
        deleting={remove.isPending}
        onReplay={replay}
        onDelete={(entry) => {
          remove.mutate(entry.id);
        }}
      />
    </div>
  );
}
