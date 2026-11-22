import { useQueryClient } from '@tanstack/react-query';
import { useEffect, useRef, useState } from 'react';
import { useTranslation } from 'react-i18next';
import { storeEndpointInCache, useEndpoint, useInstance, withoutViewGrant } from '../../api/queries';
import type { Endpoint } from '../../api/types';
import { ConflictBanner } from '../../components/ConflictBanner/ConflictBanner';
import { clearGrant } from './grants';
import { DraftFooter } from '../workspace/DraftFooter';
import type { SectionProps } from '../workspace/sections';
import { useSettingsDraft } from '../workspace/useSettingsDraft';
import { ApiError } from '../../api/client';
import { adoptViewGrant, patchAccess } from './accessApi';
import {
  accessDraftOf,
  buildAccessPatch,
  isAccessValid,
  isPatchEmpty,
  issuesFromServer,
  serializeAccessDraft,
  validateAccess,
  withoutSecrets,
} from './accessDraft';
import { CaptureAuthSetting } from './CaptureAuthSetting';
import { useAccessIssueText } from './issueText';
import { MembersSection } from './MembersSection';
import { PasswordSetting } from './PasswordSetting';
import { SendersSetting } from './SendersSetting';
import { readAccess, readClientIp } from './snapshot';
import type { AccessDraft, AccessIssues } from './types';
import styles from './access.module.css';

const NO_ISSUES: AccessIssues = {};

function draftOfEndpoint(endpoint: Endpoint): AccessDraft {
  return accessDraftOf(readAccess(endpoint));
}

const CLIENT_ERROR_MIN = 400;
const SERVER_ERROR_MIN = 500;

function isDefinitiveRefusal(failure: unknown): boolean {
  return failure instanceof ApiError && failure.status >= CLIENT_ERROR_MIN && failure.status < SERVER_ERROR_MIN;
}

function ignoreDraft(): void {
  return undefined;
}

export function AccessSection({ endpointId }: SectionProps) {
  const { t } = useTranslation();
  const issueText = useAccessIssueText();
  const queryClient = useQueryClient();
  const endpointQuery = useEndpoint(endpointId);
  const endpoint = endpointQuery.data;
  const instance = useInstance().data;
  const [serverIssues, setServerIssues] = useState<AccessIssues>(NO_ISSUES);
  const setDraftRef = useRef<(draft: AccessDraft) => void>(ignoreDraft);
  const snapshot = endpoint === undefined ? null : readAccess(endpoint);
  const canEdit = endpoint !== undefined && (endpoint.my_role === null || endpoint.my_role === 'owner');

  async function writeAccess(draft: AccessDraft): Promise<Endpoint> {
    if (endpoint === undefined || snapshot === null) {
      throw new Error('The endpoint is not loaded.');
    }
    const patch = buildAccessPatch(draft, snapshot);
    if (isPatchEmpty(patch)) {
      return endpoint;
    }
    setServerIssues(NO_ISSUES);
    try {
      const saved = await patchAccess(endpointId, patch);
      if (typeof patch.view_password === 'string') {
        await adoptViewGrant(endpointId, saved, patch.view_password);
      } else if (patch.view_password === null) {
        clearGrant(endpointId);
      }
      storeEndpointInCache(queryClient, saved);
      return withoutViewGrant(saved);
    } catch (failure: unknown) {
      if (isDefinitiveRefusal(failure)) {
        setServerIssues(issuesFromServer(failure));
        setDraftRef.current(withoutSecrets(draft));
      } else {
        setServerIssues({ general: { reason: 'unknownOutcome' } });
      }
      throw failure;
    }
  }

  const settings = useSettingsDraft<AccessDraft, Endpoint>({
    source: endpoint,
    toDraft: draftOfEndpoint,
    serialize: serializeAccessDraft,
    refresh: async () => (await endpointQuery.refetch()).data,
    write: writeAccess,
    validate: (draft) => snapshot !== null && isAccessValid(draft, snapshot),
    readOnly: !canEdit,
  });
  const { setDraft } = settings;

  useEffect(() => {
    setDraftRef.current = setDraft;
  }, [setDraft]);

  const draft = settings.draft;
  if (endpoint === undefined || snapshot === null || draft === null || instance === undefined) {
    return (
      <p role="status" className={styles.loading}>
        {t('wl.workspace.loading')}
      </p>
    );
  }

  function update(next: AccessDraft): void {
    setServerIssues(NO_ISSUES);
    setDraft(next);
  }

  const clientIssues = settings.attempts > 0 && snapshot !== null ? validateAccess(draft, snapshot) : NO_ISSUES;
  const issues: AccessIssues = { ...clientIssues, ...serverIssues };
  const owner = endpoint.owner;
  const showPassword = owner === null;
  const showMembers = owner !== null && instance.auth_mode !== 'open';

  return (
    <div className={styles.page}>
      <h2 className={styles.heading}>{t('wl.workspace.section.access')}</h2>
      {settings.conflict ? <ConflictBanner onReload={settings.reload} onOverwrite={settings.overwrite} /> : null}
      {canEdit ? null : <p className={styles.hint}>{t('wl.access.readOnly')}</p>}
      <div className={styles.form}>
        {showPassword ? (
          <PasswordSetting
            draft={draft}
            isSet={snapshot.viewProtected}
            readOnly={!canEdit}
            issue={issues.password ?? null}
            onChange={update}
          />
        ) : null}
        <SendersSetting
          draft={draft}
          clientIp={readClientIp(instance)}
          readOnly={!canEdit}
          issue={issues.cidrs ?? null}
          onChange={update}
        />
        <CaptureAuthSetting
          draft={draft}
          current={snapshot.captureAuth}
          readOnly={!canEdit}
          nameIssue={issues.authName ?? null}
          secretIssue={issues.authSecret ?? null}
          onChange={update}
        />
      </div>
      {issues.general === undefined ? null : (
        <p role="alert" className={styles.error}>
          {issueText(issues.general)}
        </p>
      )}
      {showMembers && owner !== null ? (
        <MembersSection
          endpointId={endpointId}
          owner={owner}
          canManage={endpoint.my_role === 'owner'}
          myUserId={instance.user?.id ?? null}
        />
      ) : null}
      <DraftFooter status={settings} readOnly={!canEdit} />
    </div>
  );
}
