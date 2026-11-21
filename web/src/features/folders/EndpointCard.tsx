import { Link } from '@tanstack/react-router';
import { useId, useState } from 'react';
import { useTranslation } from 'react-i18next';
import { ApiError } from '../../api/client';
import { useUpdateEndpoint } from '../../api/queries';
import { signInHref } from '../../app/signIn';
import { ConfirmDialog } from '../../components/ConfirmDialog/ConfirmDialog';
import { forgetEndpoint } from '../endpoints/store';
import { formatRelativeTime, shortEndpointId } from '../endpoints/format';
import { EndpointTags } from './EndpointTags';
import { useDeleteOwnedEndpoint, useMoveAccountEndpoint, useMoveToAccount, type MoveToAccountResult } from './api';
import { forgetLocalEndpointOrganize, placeLocalEndpointIn, setLocalEndpointTags } from './organizeStore';
import { folderIdFromKey, rowName, type FolderView, type OrganizedEndpoint } from './model';
import type { SessionState } from './useOrganizeData';
import styles from './EndpointCard.module.css';

export interface EndpointCardProps {
  row: OrganizedEndpoint;
  folders: readonly FolderView[];
  suggestions: readonly string[];
  session: SessionState;
  now: number;
  locale: string;
  onSettled: (message: string) => void;
}

export function EndpointCard({ row, folders, suggestions, session, now, locale, onSettled }: EndpointCardProps) {
  const { t } = useTranslation();
  const folderFieldId = useId();
  const [error, setError] = useState<string | null>(null);
  const [confirmDelete, setConfirmDelete] = useState(false);
  const name = rowName(row);
  const isLocal = row.source === 'local';
  const isOwner = row.source === 'account';
  const readOnlyTags = row.source === 'shared' && row.role !== 'editor';
  const folderOptions = folders.filter((folder) => (isLocal ? folder.source === 'local' : folder.source === 'account'));
  const currentFolder = folders.find((folder) => folder.key === row.folderKey);
  const updateTags = useUpdateEndpoint(row.id);
  const moveFolder = useMoveAccountEndpoint();
  const moveToAccount = useMoveToAccount((result: MoveToAccountResult) =>
    onSettled(result.detailsSaved ? t('wl.rail.row.moved') : t('wl.rail.row.movedDetailsFailed')),
  );
  const deleteOwned = useDeleteOwnedEndpoint(() => onSettled(t('wl.rail.row.deleted')));

  function claimFailureText(failure: Error): string {
    const code = failure instanceof ApiError ? failure.code : null;
    switch (code) {
      case 'claim_refused':
        return t('wl.rail.move.refused');
      case 'endpoint_gone':
        return t('wl.rail.move.gone');
      case 'sign_in_required':
        return t('wl.rail.move.signIn');
      default:
        return t('wl.rail.move.failed');
    }
  }

  function sourceText(): string {
    switch (row.source) {
      case 'local':
        return t('wl.rail.source.local');
      case 'account':
        return t('wl.rail.source.account');
      case 'shared':
        return row.role === 'editor' ? t('wl.rail.source.sharedEditor') : t('wl.rail.source.sharedViewer');
    }
  }

  function changeTags(tags: string[]) {
    setError(null);
    if (isLocal) {
      setLocalEndpointTags(row.id, tags);
      return;
    }
    updateTags.mutate({ tags }, { onError: () => setError(t('wl.rail.row.tagsFailed')) });
  }

  function changeFolder(key: string) {
    setError(null);
    if (isLocal) {
      placeLocalEndpointIn(row.id, key === '' ? null : folderIdFromKey(key));
      return;
    }
    moveFolder.mutate(
      {
        endpointId: row.id,
        fromFolderId: row.folderKey === null ? null : folderIdFromKey(row.folderKey),
        toFolderId: key === '' ? null : folderIdFromKey(key),
      },
      { onError: () => setError(t('wl.rail.row.folderFailed')) },
    );
  }

  function removeFromList() {
    forgetEndpoint(row.id);
    forgetLocalEndpointOrganize(row.id);
    onSettled(t('wl.rail.row.removed'));
  }

  function moveRow() {
    setError(null);
    moveToAccount.mutate(
      {
        endpointId: row.id,
        label: row.label,
        tags: row.tags,
        folderName: currentFolder?.name ?? null,
      },
      {
        onError: (failure) => setError(claimFailureText(failure)),
      },
    );
  }

  return (
    <li className={styles.card}>
      <div className={styles.head}>
        <Link to="/e/$id" params={{ id: row.id }} className={styles.name}>
          {name}
        </Link>
        <span className={styles.source}>{sourceText()}</span>
      </div>
      <p className={styles.meta}>
        <span className={styles.mono}>{shortEndpointId(row.id)}</span>
        {row.captureCount === null ? null : <span>{t('wl.rail.row.captures', { count: row.captureCount })}</span>}
        <span>{t('wl.rail.row.lastActivity', { time: formatRelativeTime(row.lastActivity, now, locale) })}</span>
      </p>
      <div className={styles.field}>
        <label htmlFor={folderFieldId} className={styles.fieldLabel}>
          {t('wl.rail.row.folderLabel', { name })}
        </label>
        <select
          id={folderFieldId}
          className={styles.select}
          value={row.folderKey ?? ''}
          disabled={moveFolder.isPending}
          onChange={(event) => changeFolder(event.target.value)}
        >
          <option value="">{t('wl.rail.folders.unfiled')}</option>
          {folderOptions.map((folder) => (
            <option key={folder.key} value={folder.key}>
              {folder.name}
            </option>
          ))}
        </select>
      </div>
      <EndpointTags
        name={name}
        tags={row.tags}
        suggestions={suggestions}
        readOnly={readOnlyTags}
        onChange={changeTags}
      />
      <div className={styles.actions}>
        {isLocal && session.signedIn ? (
          <button
            type="button"
            className={styles.action}
            aria-label={t('wl.rail.row.moveLabel', { name })}
            disabled={moveToAccount.isPending}
            onClick={moveRow}
          >
            {t('wl.rail.row.moveToAccount')}
          </button>
        ) : null}
        {isLocal && !session.signedIn && session.signInAvailable ? (
          <a className={styles.action} href={signInHref('/endpoints')}>
            {t('wl.rail.row.signInToMove')}
          </a>
        ) : null}
        {isLocal ? (
          <button
            type="button"
            className={styles.action}
            aria-label={t('wl.rail.row.removeLabel', { name })}
            onClick={removeFromList}
          >
            {t('wl.rail.row.removeFromList')}
          </button>
        ) : null}
        {isOwner ? (
          <button
            type="button"
            className={styles.danger}
            aria-label={t('wl.rail.row.deleteLabel', { name })}
            onClick={() => setConfirmDelete(true)}
          >
            {t('wl.rail.row.delete')}
          </button>
        ) : null}
      </div>
      {error === null ? null : (
        <p className={styles.error} role="alert">
          {error}
        </p>
      )}
      {isOwner ? (
        <ConfirmDialog
          open={confirmDelete}
          title={t('wl.rail.row.delete.title', { name })}
          body={t('wl.rail.row.delete.body')}
          confirmLabel={t('wl.rail.row.delete.confirm')}
          cancelLabel={t('wl.rail.folders.cancel')}
          busy={deleteOwned.isPending}
          onConfirm={() => deleteOwned.mutate(row.id)}
          onOpenChange={setConfirmDelete}
        />
      ) : null}
    </li>
  );
}
