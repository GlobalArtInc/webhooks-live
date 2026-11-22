import { Link } from '@tanstack/react-router';
import { useId, useRef, useState } from 'react';
import { useTranslation } from 'react-i18next';
import { ConfirmDialog } from '../../components/ConfirmDialog/ConfirmDialog';
import { ApiError } from '../../api/client';
import { useRailState } from '../../app/shell/railState';
import { markEndpointOpened } from '../endpoints/store';
import { FolderForm } from './FolderForm';
import { useCreateAccountFolder, useDeleteAccountFolder, useRenameAccountFolder } from './api';
import { MAX_LOCAL_FOLDERS } from './localOrganize';
import {
  groupByFolder,
  labelCounts,
  rowName,
  type FolderSource,
  type FolderView,
  type OrganizedEndpoint,
  type RailGroup,
} from './model';
import { createLocalFolder, deleteLocalFolder, renameLocalFolderName } from './organizeStore';
import { useOrganizeData } from './useOrganizeData';
import styles from './FolderRail.module.css';

export interface FolderRailProps {
  listId: string;
}

interface RailActions {
  renaming: string | null;
  deleting: FolderView | null;
  setRenaming: (key: string | null) => void;
  setDeleting: (folder: FolderView | null) => void;
  rename: (folder: FolderView, name: string) => void;
  remove: (folder: FolderView) => void;
  namesFor: (source: FolderSource, exceptId: string | null) => string[];
  busy: boolean;
}

interface EndpointLinkProps {
  row: OrganizedEndpoint;
}

interface FolderGroupProps {
  group: RailGroup;
  actions: RailActions;
}

function EndpointLink({ row }: EndpointLinkProps) {
  const { closeDrawer } = useRailState();
  return (
    <Link
      to="/e/$id"
      params={{ id: row.id }}
      className={styles.link}
      activeProps={{ className: styles.active }}
      title={rowName(row)}
      onClick={() => {
        if (row.source === 'local') {
          markEndpointOpened(row.id);
        }
        closeDrawer();
      }}
    >
      <span className={styles.linkText}>{rowName(row)}</span>
    </Link>
  );
}

function FolderGroup({ group, actions }: FolderGroupProps) {
  const { t } = useTranslation();
  const [expanded, setExpanded] = useState(true);
  const listId = useId();
  const folder = group.folder;
  const name = folder === null ? t('wl.rail.folders.unfiled') : folder.name;
  const renameKey = folder === null ? null : folder.key;
  const count = group.rows.length;

  return (
    <div className={styles.group}>
      <div className={styles.groupHead}>
        <button
          type="button"
          className={styles.groupToggle}
          aria-expanded={expanded}
          aria-controls={listId}
          onClick={() => setExpanded((current) => !current)}
        >
          <span className={styles.groupName}>{name}</span>
          <span className={styles.count}>{t('wl.rail.folders.count', { count })}</span>
        </button>
        {folder === null ? null : (
          <div className={styles.groupActions}>
            <button
              type="button"
              className={styles.small}
              aria-label={t('wl.rail.folders.renameLabel', { name })}
              onClick={() => actions.setRenaming(renameKey)}
            >
              {t('wl.rail.folders.rename')}
            </button>
            <button
              type="button"
              className={styles.small}
              aria-label={t('wl.rail.folders.deleteLabel', { name })}
              onClick={() => actions.setDeleting(folder)}
            >
              {t('wl.rail.folders.delete')}
            </button>
          </div>
        )}
      </div>
      {folder !== null && actions.renaming === renameKey ? (
        <FolderForm
          initialName={folder.name}
          initialSource={folder.source}
          showSource={false}
          submitLabel={t('wl.rail.folders.save')}
          busy={actions.busy}
          takenNames={(source) => actions.namesFor(source, folder.id)}
          onSubmit={(next) => actions.rename(folder, next)}
          onCancel={() => actions.setRenaming(null)}
        />
      ) : null}
      {expanded ? (
        <ul id={listId} className={styles.list}>
          {group.rows.length === 0 ? <li className={styles.empty}>{t('wl.rail.folders.empty')}</li> : null}
          {group.rows.map((row) => (
            <li key={`${row.source}-${row.id}`}>
              <EndpointLink row={row} />
            </li>
          ))}
        </ul>
      ) : null}
    </div>
  );
}

export function FolderRail({ listId }: FolderRailProps) {
  const { t } = useTranslation();
  const data = useOrganizeData({});
  const headingId = useId();
  const labelsHeadingId = useId();
  const newFolderRef = useRef<HTMLButtonElement>(null);
  const [creating, setCreating] = useState(false);
  const [renaming, setRenaming] = useState<string | null>(null);
  const [deleting, setDeleting] = useState<FolderView | null>(null);
  const [notice, setNotice] = useState<string | null>(null);
  const createAccount = useCreateAccountFolder();
  const renameAccount = useRenameAccountFolder();
  const deleteAccount = useDeleteAccountFolder();
  const { session, organize, accountFolders } = data;

  function namesFor(source: FolderSource, exceptId: string | null): string[] {
    if (source === 'account') {
      return accountFolders.filter((folder) => folder.id !== exceptId).map((folder) => folder.name);
    }
    return organize.folders.filter((folder) => folder.id !== exceptId).map((folder) => folder.name);
  }

  function errorText(error: unknown): string {
    if (error instanceof ApiError && error.code === 'name_taken') {
      return t('wl.rail.folders.nameError.duplicate');
    }
    if (error instanceof ApiError && error.code === 'limit_reached') {
      return t('wl.rail.folders.error.limit');
    }
    return t('wl.rail.folders.error.failed');
  }

  function create(name: string, source: FolderSource) {
    setNotice(null);
    if (source === 'local') {
      if (organize.folders.length >= MAX_LOCAL_FOLDERS) {
        setNotice(t('wl.rail.folders.error.limit'));
        return;
      }
      createLocalFolder(name);
      setCreating(false);
      newFolderRef.current?.focus();
      return;
    }
    createAccount.mutate(name, {
      onSuccess: () => {
        setCreating(false);
        newFolderRef.current?.focus();
      },
      onError: (error) => setNotice(errorText(error)),
    });
  }

  function rename(folder: FolderView, name: string) {
    setNotice(null);
    if (folder.source === 'local') {
      renameLocalFolderName(folder.id, name);
      setRenaming(null);
      return;
    }
    renameAccount.mutate(
      { folderId: folder.id, name },
      {
        onSuccess: () => setRenaming(null),
        onError: (error) => setNotice(errorText(error)),
      },
    );
  }

  function remove(folder: FolderView) {
    setNotice(null);
    if (folder.source === 'local') {
      deleteLocalFolder(folder.id);
      setDeleting(null);
      newFolderRef.current?.focus();
      return;
    }
    deleteAccount.mutate(folder.id, {
      onSuccess: () => {
        setDeleting(null);
        newFolderRef.current?.focus();
      },
      onError: (error) => {
        setDeleting(null);
        setNotice(errorText(error));
      },
    });
  }

  const actions: RailActions = {
    renaming,
    deleting,
    setRenaming,
    setDeleting,
    rename,
    remove,
    namesFor,
    busy: renameAccount.isPending,
  };
  const groups = groupByFolder(data.allRows, data.folders);
  const labels = labelCounts(data.allRows);
  const folderSourceDefault: FolderSource = session.signedIn ? 'account' : 'local';

  return (
    <div id={listId} className={styles.organizer}>
      <section aria-labelledby={headingId} className={styles.section}>
        <div className={styles.sectionHead}>
          <h3 id={headingId} className={styles.heading}>
            {t('wl.rail.folders.heading')}
          </h3>
          <button
            ref={newFolderRef}
            type="button"
            className={styles.small}
            aria-expanded={creating}
            onClick={() => setCreating((current) => !current)}
          >
            {t('wl.rail.folders.new')}
          </button>
        </div>
        {creating ? (
          <FolderForm
            initialName=""
            initialSource={folderSourceDefault}
            showSource={session.signedIn}
            submitLabel={t('wl.rail.folders.create')}
            busy={createAccount.isPending}
            takenNames={(source) => namesFor(source, null)}
            onSubmit={create}
            onCancel={() => setCreating(false)}
          />
        ) : null}
        {notice === null ? null : (
          <p className={styles.error} role="alert">
            {notice}
          </p>
        )}
        {data.allRows.length === 0 ? <p className={styles.empty}>{t('wl.home.rail.empty')}</p> : null}
        {groups.map((group) => (
          <FolderGroup key={group.folder?.key ?? 'unfiled'} group={group} actions={actions} />
        ))}
        {data.failed ? (
          <p className={styles.error} role="alert">
            {t('wl.rail.folders.loadFailed')}
          </p>
        ) : null}
      </section>
      <section aria-labelledby={labelsHeadingId} className={styles.section}>
        <h3 id={labelsHeadingId} className={styles.heading}>
          {t('wl.rail.labels.heading')}
        </h3>
        {labels.length === 0 ? <p className={styles.empty}>{t('wl.rail.labels.empty')}</p> : null}
        {labels.length === 0 ? null : (
          <ul className={styles.labels}>
            {labels.map(({ label, count }) => (
              <li key={label}>
                <Link to="/endpoints" search={{ label }} className={styles.labelChip}>
                  <span>{label}</span>
                  <span className={styles.count}>{count}</span>
                </Link>
              </li>
            ))}
          </ul>
        )}
      </section>
      <ConfirmDialog
        open={deleting !== null}
        title={t('wl.rail.folders.delete.title', { name: deleting?.name ?? '' })}
        body={t('wl.rail.folders.delete.body')}
        confirmLabel={t('wl.rail.folders.delete.confirm')}
        cancelLabel={t('wl.rail.folders.cancel')}
        busy={deleteAccount.isPending}
        onConfirm={() => {
          if (deleting !== null) {
            remove(deleting);
          }
        }}
        onOpenChange={(open) => {
          if (!open) {
            setDeleting(null);
          }
        }}
      />
    </div>
  );
}
