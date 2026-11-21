import { useRef, useState } from 'react';
import { useTranslation } from 'react-i18next';
import { StatePage } from '../../components/StatePage/StatePage';
import { stateKindForError } from '../../components/StatePage/stateKind';
import { ConfirmDialog } from '../../components/ConfirmDialog/ConfirmDialog';
import { ApiError } from '../../api/client';
import { useApiTokens, useRevokeApiToken, type ApiToken } from '../../api/tokens';
import { signInHref } from '../../app/signIn';
import { CreateTokenDialog } from './CreateTokenDialog';
import { TokenTable } from './TokenTable';
import styles from './TokensPage.module.css';

export interface TokensPageProps {
  returnTo: string;
  providerName: string | null;
}

const SESSION_CODES: readonly string[] = ['sign_in_required', 'unauthenticated', 'session_required'];

function isSessionError(error: unknown): boolean {
  return error instanceof ApiError && error.code !== null && SESSION_CODES.includes(error.code);
}

export function TokensPage({ returnTo, providerName }: TokensPageProps) {
  const { t } = useTranslation();
  const tokens = useApiTokens();
  const revoke = useRevokeApiToken();
  const headingRef = useRef<HTMLHeadingElement>(null);
  const focusHeadingOnClose = useRef(false);
  const [creating, setCreating] = useState(false);
  const [target, setTarget] = useState<ApiToken | null>(null);
  const [notice, setNotice] = useState<string | null>(null);
  const [revokeFailed, setRevokeFailed] = useState(false);

  if (tokens.isError && tokens.data === undefined) {
    if (isSessionError(tokens.error)) {
      return <StatePage kind="signin" detail={{ providerName, signInHref: signInHref(returnTo) }} />;
    }
    return (
      <StatePage
        kind={stateKindForError(tokens.error)}
        detail={tokens.error instanceof ApiError ? { status: tokens.error.status } : {}}
        onRetry={() => void tokens.refetch()}
      />
    );
  }

  function confirmRevoke() {
    if (target === null) {
      return;
    }
    const revoked = target;
    setRevokeFailed(false);
    revoke.mutate(revoked.id, {
      onSuccess: () => {
        focusHeadingOnClose.current = true;
        setTarget(null);
        setNotice(t('wl.tokens.revoke.done', { name: revoked.name }));
      },
      onError: () => {
        focusHeadingOnClose.current = true;
        setTarget(null);
        setNotice(null);
        setRevokeFailed(true);
      },
    });
  }

  const items = tokens.data?.items;

  return (
    <section className={styles.page} aria-labelledby="tokens-title">
      <header className={styles.header}>
        <div className={styles.heading}>
          <h1 id="tokens-title" ref={headingRef} tabIndex={-1} className={styles.title}>
            {t('wl.tokens.title')}
          </h1>
          <p className={styles.description}>{t('wl.tokens.description')}</p>
        </div>
        <button type="button" className={styles.create} onClick={() => setCreating(true)}>
          {t('wl.tokens.create')}
        </button>
      </header>

      <p role="status" className={styles.status}>
        {notice}
      </p>
      {revokeFailed && (
        <p role="alert" className={styles.failure}>
          {t('wl.tokens.revoke.failed')}
        </p>
      )}

      {items === undefined && (
        <p role="status" className={styles.loading}>
          {t('wl.tokens.loading')}
        </p>
      )}
      {items !== undefined && items.length === 0 && (
        <div className={styles.empty}>
          <h2 className={styles.emptyTitle}>{t('wl.tokens.empty.title')}</h2>
          <p className={styles.description}>{t('wl.tokens.empty.body')}</p>
        </div>
      )}
      {items !== undefined && items.length > 0 && <TokenTable tokens={items} onRevoke={setTarget} />}

      {creating && <CreateTokenDialog onClose={() => setCreating(false)} />}

      <ConfirmDialog
        open={target !== null}
        title={t('wl.tokens.revoke.title', { name: target?.name ?? '' })}
        body={t('wl.tokens.revoke.confirm')}
        confirmLabel={t('wl.tokens.revoke.action')}
        cancelLabel={t('wl.tokens.revoke.cancel')}
        busy={revoke.isPending}
        onConfirm={confirmRevoke}
        onOpenChange={(open) => {
          if (!open && !revoke.isPending) {
            setTarget(null);
          }
        }}
        onCloseAutoFocus={(event) => {
          if (focusHeadingOnClose.current) {
            focusHeadingOnClose.current = false;
            event.preventDefault();
            headingRef.current?.focus();
          }
        }}
      />
    </section>
  );
}
