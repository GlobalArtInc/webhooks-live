import { useNavigate } from '@tanstack/react-router';
import { useId, useState } from 'react';
import { useTranslation } from 'react-i18next';
import { useEndpoint, useInstance, useUpdateEndpoint } from '../../../api/queries';
import type { Endpoint } from '../../../api/types';
import { ConflictBanner } from '../../../components/ConflictBanner/ConflictBanner';
import { useAccountFolders, useMoveAccountEndpoint } from '../../folders/api';
import { EndpointTags } from '../../folders/EndpointTags';
import { AliasesField } from '../../general/AliasesField';
import { CloneDialog } from '../../general/CloneDialog';
import { FolderSelect } from '../../general/FolderSelect';
import { LifetimeField } from '../../general/LifetimeField';
import {
  draftOf,
  isCaptureTtlValid,
  isLifetimeValid,
  isMaxCapturesValid,
  limitsOf,
  patchFor,
  type GeneralDraft,
  type GeneralLimits,
} from '../../general/model';
import { UsageMeters } from '../../general/UsageMeters';
import type { SectionProps } from '../sections';
import { DraftFooter } from '../DraftFooter';
import { useSettingsDraft } from '../useSettingsDraft';
import { DangerZone } from './DangerZone';
import styles from './GeneralSection.module.css';

export const TITLE_MAX_CHARS = 100;

function serializeDraft(draft: GeneralDraft): string {
  return JSON.stringify(draft);
}

function isTitleValid(draft: GeneralDraft): boolean {
  return draft.title.trim().length <= TITLE_MAX_CHARS;
}

function isDraftValid(draft: GeneralDraft, limits: GeneralLimits): boolean {
  return (
    isTitleValid(draft) &&
    isLifetimeValid(draft, limits) &&
    isMaxCapturesValid(draft.maxCaptures, limits) &&
    isCaptureTtlValid(draft.captureTtl)
  );
}

export function GeneralSection({ endpointId }: SectionProps) {
  const { t } = useTranslation();
  const navigate = useNavigate();
  const titleId = useId();
  const endpointQuery = useEndpoint(endpointId);
  const endpoint = endpointQuery.data;
  const instance = useInstance().data;
  const signedIn = Boolean(instance?.user);
  const limits = limitsOf(instance);
  const folders = useAccountFolders(signedIn);
  const moveEndpoint = useMoveAccountEndpoint();
  const update = useUpdateEndpoint(endpointId);
  const [cloneOpen, setCloneOpen] = useState(false);
  const [folderFailed, setFolderFailed] = useState(false);
  const readOnly = endpoint?.my_role === 'viewer';
  const isOwner = endpoint?.my_role === 'owner';
  const settings = useSettingsDraft<GeneralDraft, Endpoint>({
    source: endpoint,
    toDraft: draftOf,
    serialize: serializeDraft,
    refresh: async () => (await endpointQuery.refetch()).data,
    write: (draft) => {
      if (endpoint === undefined) {
        return Promise.reject(new Error('The endpoint is not loaded.'));
      }
      return update.mutateAsync(patchFor(draft, endpoint, isOwner));
    },
    validate: (draft) => isDraftValid(draft, limits),
    readOnly,
  });

  if (endpoint === undefined || settings.draft === null) {
    return (
      <p role="status" className={styles.loading}>
        {t('wl.workspace.loading')}
      </p>
    );
  }

  const draft = settings.draft;
  const tooLong = !isTitleValid(draft);
  const titleErrorId = `${titleId}-error`;
  const currentFolder = folders.data?.find((folder) => folder.endpoint_ids.includes(endpointId))?.id ?? null;

  function moveToFolder(folderId: string | null): void {
    setFolderFailed(false);
    moveEndpoint.mutate(
      { endpointId, fromFolderId: currentFolder, toFolderId: folderId },
      {
        onError: () => {
          setFolderFailed(true);
        },
      },
    );
  }

  return (
    <div className={styles.page}>
      <h2 className={styles.heading}>{t('wl.workspace.section.general')}</h2>
      {settings.conflict ? <ConflictBanner onReload={settings.reload} onOverwrite={settings.overwrite} /> : null}
      <fieldset className={styles.form} disabled={readOnly}>
        <div className={styles.field}>
          <label htmlFor={titleId} className={styles.label}>
            {t('wl.general.title.label')}
          </label>
          <input
            id={titleId}
            type="text"
            className={styles.input}
            value={draft.title}
            aria-invalid={tooLong ? true : undefined}
            aria-describedby={tooLong ? titleErrorId : undefined}
            onChange={(event) => {
              settings.setDraft({ ...draft, title: event.target.value });
            }}
          />
          {tooLong ? (
            <p id={titleErrorId} className={styles.error}>
              {t('wl.general.title.tooLong', { limit: TITLE_MAX_CHARS })}
            </p>
          ) : null}
        </div>
        {signedIn ? (
          <FolderSelect
            folders={folders.data ?? []}
            value={currentFolder}
            disabled={folders.data === undefined || moveEndpoint.isPending}
            onChange={moveToFolder}
          />
        ) : (
          <p className={styles.note}>{t('wl.general.folder.signIn')}</p>
        )}
        {folderFailed ? (
          <p role="alert" className={styles.error}>
            {t('wl.general.folder.failed')}
          </p>
        ) : null}
        <EndpointTags
          name={endpoint.title ?? endpoint.id}
          tags={draft.tags}
          suggestions={[]}
          readOnly={readOnly}
          onChange={(tags) => {
            settings.setDraft({ ...draft, tags });
          }}
        />
        <LifetimeField
          draft={draft}
          limits={limits}
          readOnly={!isOwner}
          expiresAt={endpoint.retention?.mode === 'never' ? null : endpoint.expires_at}
          onChange={settings.setDraft}
        />
      </fieldset>
      <AliasesField endpointId={endpointId} readOnly={!isOwner} />
      <UsageMeters
        captureCount={endpoint.capture_count}
        maxCaptures={endpoint.max_captures}
        bytesUsed={endpoint.bytes_used}
      />
      <div className={styles.actions}>
        <button
          type="button"
          className={styles.secondary}
          onClick={() => {
            setCloneOpen(true);
          }}
        >
          {t('wl.general.clone.open')}
        </button>
      </div>
      {cloneOpen ? (
        <CloneDialog
          endpointId={endpointId}
          title={endpoint.title}
          onClose={() => {
            setCloneOpen(false);
          }}
          onCloned={(clone) => {
            setCloneOpen(false);
            void navigate({ to: '/e/$id/settings/$section', params: { id: clone.id, section: 'general' } });
          }}
        />
      ) : null}
      <DangerZone endpointId={endpointId} captureCount={endpoint.capture_count} myRole={endpoint.my_role} />
      <DraftFooter
        status={settings}
        readOnly={readOnly}
        saveBlockedText={isDraftValid(draft, limits) ? undefined : t('wl.general.saveBlocked')}
      />
    </div>
  );
}
