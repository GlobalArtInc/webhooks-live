import { useEffect, useId, useMemo, useRef, useState } from 'react';
import { useTranslation } from 'react-i18next';
import { offersSecrets } from '../../api/capabilities';
import { ApiError } from '../../api/client';
import { useInstance } from '../../api/queries';
import { ConfirmDialog } from '../../components/ConfirmDialog/ConfirmDialog';
import { ConflictBanner } from '../../components/ConflictBanner/ConflictBanner';
import { DraftFooter } from '../workspace/DraftFooter';
import { useSettingsDraft } from '../workspace/useSettingsDraft';
import { KindName } from './ChannelTable';
import {
  blankDraft,
  channelInputOf,
  draftOf,
  issuesFromServer,
  NO_ISSUES,
  serializeDraft,
  validateDraft,
  type ChannelDraft,
  type IssueField,
  type IssueMap,
} from './channelDraft';
import { fieldsOf, isChannelKind } from './kinds';
import { useIssueText, useErrorText } from './issueText';
import { PushSetting } from './PushSetting';
import { useChannels, useCreateChannel, useDeleteChannel, useSecrets, useUpdateChannel } from './notificationsQueries';
import type { Channel, ChannelKind, EmailAddress, SecretSummary } from './types';
import styles from './ChannelEditor.module.css';

export interface ChannelEditorProps {
  endpointId: string;
  channel: Channel | null;
  kinds: readonly ChannelKind[];
  readOnly: boolean;
  emailAddresses: readonly EmailAddress[];
  signedIn: boolean;
  onCreated: (channelId: string) => void;
  onDeleted: () => void;
}

function keepDraft(draft: ChannelDraft): ChannelDraft {
  return draft;
}

function focusFirstInvalid(form: HTMLElement | null): void {
  form?.querySelector<HTMLElement>('[aria-invalid="true"]')?.focus();
}

export function ChannelEditor({
  endpointId,
  channel,
  kinds,
  readOnly,
  emailAddresses,
  signedIn,
  onCreated,
  onDeleted,
}: ChannelEditorProps) {
  const { t } = useTranslation();
  const baseId = useId();
  const formRef = useRef<HTMLFieldSetElement>(null);
  const issueText = useIssueText();
  const errorText = useErrorText();
  const listQuery = useChannels(endpointId, true);
  const instance = useInstance().data;
  const secretsQuery = useSecrets(endpointId, offersSecrets(instance));
  const create = useCreateChannel(endpointId);
  const update = useUpdateChannel(endpointId);
  const remove = useDeleteChannel(endpointId);
  const [confirmDelete, setConfirmDelete] = useState(false);
  const channelId = channel === null ? null : channel.id;
  const firstKind: ChannelKind = kinds[0] ?? 'webhook';
  const source = useMemo(() => (channel === null ? blankDraft(firstKind) : draftOf(channel)), [channel, firstKind]);

  async function writeDraft(draft: ChannelDraft): Promise<ChannelDraft> {
    if (channelId === null) {
      const created = await create.mutateAsync(channelInputOf(draft, true));
      onCreated(created.id);
      return draftOf(created);
    }
    const saved = await update.mutateAsync({ channelId, input: channelInputOf(draft, false) });
    return draftOf(saved);
  }

  const settings = useSettingsDraft<ChannelDraft, ChannelDraft>({
    source,
    toDraft: keepDraft,
    serialize: serializeDraft,
    refresh: async () => {
      if (channelId === null) {
        return undefined;
      }
      const result = await listQuery.refetch();
      const found = result.data?.items.find((item) => item.id === channelId);
      return found === undefined ? undefined : draftOf(found);
    },
    write: writeDraft,
    validate: (draft) => validateDraft(draft, channel === null).size === 0,
    readOnly,
  });
  const { attempts } = settings;
  const draft = settings.draft;

  useEffect(() => {
    if (attempts > 0) {
      focusFirstInvalid(formRef.current);
    }
  }, [attempts]);

  if (draft === null) {
    return (
      <p role="status" className={styles.muted}>
        {t('wl.workspace.loading')}
      </p>
    );
  }

  const isNew = channel === null;
  const clientIssues = validateDraft(draft, isNew);
  const hasFieldErrors = settings.error instanceof ApiError && settings.error.details.length > 0;
  const serverIssues: IssueMap =
    settings.error instanceof ApiError && settings.error.details.length > 0
      ? issuesFromServer(settings.error.details)
      : NO_ISSUES;
  const issues: IssueMap = attempts === 0 ? NO_ISSUES : clientIssues.size > 0 ? clientIssues : serverIssues;
  const saveFailure = settings.error === null || hasFieldErrors ? null : errorText(settings.error);
  const verified = emailAddresses.filter((address) => address.verified);
  const secrets: readonly SecretSummary[] = secretsQuery.data?.items ?? [];
  const kindId = `${baseId}-kind`;
  const nameId = `${baseId}-name`;
  const urlId = `${baseId}-url`;
  const secretId = `${baseId}-secret`;
  const chatId = `${baseId}-chat`;
  const addressId = `${baseId}-address`;
  const perHourId = `${baseId}-per-hour`;
  const perHourHintId = `${perHourId}-hint`;
  const urlHintId = `${urlId}-hint`;
  const kindOptions = kinds.length === 0 ? [firstKind] : kinds;
  const savedUrl = channel?.config.url ?? null;

  function issueFor(field: IssueField): string | undefined {
    const code = issues.get(field);
    return code === undefined ? undefined : issueText(code);
  }

  function errorIdFor(field: IssueField): string {
    return `${baseId}-${field}-error`;
  }

  function describedBy(field: IssueField, hintId?: string): string | undefined {
    const parts = [hintId, issues.has(field) ? errorIdFor(field) : undefined].filter(
      (part): part is string => part !== undefined,
    );
    return parts.length === 0 ? undefined : parts.join(' ');
  }

  function removeChannel(): void {
    if (channel === null) {
      return;
    }
    remove.mutate(channel.id, {
      onSuccess: () => {
        onDeleted();
      },
    });
  }

  const nameIssue = issueFor('name');
  const perHourIssue = issueFor('perHour');

  return (
    <div className={styles.page}>
      <div className={styles.header}>
        <h3 className={styles.title}>{channel === null ? t('wl.notifications.new') : channel.name}</h3>
      </div>
      {settings.conflict ? <ConflictBanner onReload={settings.reload} onOverwrite={settings.overwrite} /> : null}
      {saveFailure === null ? null : (
        <p role="alert" className={styles.error}>
          {saveFailure}
        </p>
      )}
      {remove.isError ? (
        <p role="alert" className={styles.error}>
          {errorText(remove.error)}
        </p>
      ) : null}
      <fieldset ref={formRef} className={styles.form} disabled={readOnly} aria-busy={settings.saving}>
        <div className={styles.field}>
          <label htmlFor={kindId} className={styles.label}>
            {t('wl.notifications.editor.kind')}
          </label>
          <select
            id={kindId}
            className={styles.input}
            value={draft.kind}
            disabled={!isNew}
            onChange={(event) => {
              if (isChannelKind(event.target.value)) {
                settings.setDraft({ ...draft, kind: event.target.value });
              }
            }}
          >
            {kindOptions.map((kind) => (
              <option key={kind} value={kind}>
                <KindName kind={kind} />
              </option>
            ))}
          </select>
        </div>
        <div className={styles.field}>
          <label htmlFor={nameId} className={styles.label}>
            {t('wl.notifications.editor.name')}
          </label>
          <input
            id={nameId}
            type="text"
            className={styles.input}
            aria-invalid={nameIssue === undefined ? undefined : true}
            aria-describedby={nameIssue === undefined ? undefined : errorIdFor('name')}
            value={draft.name}
            onChange={(event) => {
              settings.setDraft({ ...draft, name: event.target.value });
            }}
          />
          {nameIssue === undefined ? null : (
            <p id={errorIdFor('name')} className={styles.error}>
              {nameIssue}
            </p>
          )}
        </div>
        {fieldsOf(draft.kind).includes('url') ? (
          <div className={styles.field}>
            <label htmlFor={urlId} className={styles.label}>
              {t('wl.notifications.editor.url')}
            </label>
            <input
              id={urlId}
              type="url"
              className={styles.input}
              autoComplete="off"
              placeholder={isNew ? undefined : t('wl.notifications.editor.urlUnchanged')}
              aria-invalid={issues.has('url') ? true : undefined}
              aria-describedby={describedBy('url', isNew ? undefined : urlHintId)}
              value={draft.url}
              onChange={(event) => {
                settings.setDraft({ ...draft, url: event.target.value });
              }}
            />
            {savedUrl === null ? null : (
              <p id={urlHintId} className={styles.hint}>
                {t('wl.notifications.editor.savedUrl', { url: savedUrl })}
              </p>
            )}
            {issueFor('url') === undefined ? null : (
              <p id={errorIdFor('url')} className={styles.error}>
                {issueFor('url')}
              </p>
            )}
          </div>
        ) : null}
        {fieldsOf(draft.kind).includes('tokenSecret') ? (
          <div className={styles.field}>
            <label htmlFor={secretId} className={styles.label}>
              {t('wl.notifications.editor.tokenSecret')}
            </label>
            <select
              id={secretId}
              className={styles.input}
              aria-invalid={issues.has('tokenSecret') ? true : undefined}
              value={draft.tokenSecret}
              onChange={(event) => {
                settings.setDraft({ ...draft, tokenSecret: event.target.value });
              }}
            >
              <option value="">{t('wl.notifications.editor.chooseSecret')}</option>
              {secrets.map((secret) => (
                <option key={secret.name} value={secret.name}>
                  {secret.name}
                </option>
              ))}
            </select>
            {secrets.length === 0 ? <p className={styles.hint}>{t('wl.notifications.editor.noSecrets')}</p> : null}
            {issueFor('tokenSecret') === undefined ? null : (
              <p id={errorIdFor('tokenSecret')} className={styles.error}>
                {issueFor('tokenSecret')}
              </p>
            )}
          </div>
        ) : null}
        {fieldsOf(draft.kind).includes('chatId') ? (
          <div className={styles.field}>
            <label htmlFor={chatId} className={styles.label}>
              {t('wl.notifications.editor.chatId')}
            </label>
            <input
              id={chatId}
              type="text"
              className={styles.input}
              aria-invalid={issues.has('chatId') ? true : undefined}
              value={draft.chatId}
              onChange={(event) => {
                settings.setDraft({ ...draft, chatId: event.target.value });
              }}
            />
            {issueFor('chatId') === undefined ? null : (
              <p id={errorIdFor('chatId')} className={styles.error}>
                {issueFor('chatId')}
              </p>
            )}
          </div>
        ) : null}
        {fieldsOf(draft.kind).includes('addressId') ? (
          <div className={styles.field}>
            <label htmlFor={addressId} className={styles.label}>
              {t('wl.notifications.editor.address')}
            </label>
            <select
              id={addressId}
              className={styles.input}
              aria-invalid={issues.has('addressId') ? true : undefined}
              value={draft.addressId}
              onChange={(event) => {
                settings.setDraft({ ...draft, addressId: event.target.value });
              }}
            >
              <option value="">{t('wl.notifications.editor.chooseAddress')}</option>
              {verified.map((address) => (
                <option key={address.id} value={address.id}>
                  {address.address}
                </option>
              ))}
            </select>
            {verified.length === 0 ? <p className={styles.hint}>{t('wl.notifications.editor.noVerified')}</p> : null}
          </div>
        ) : null}
        {draft.kind === 'push' ? <PushSetting signedIn={signedIn} /> : null}
        <div className={styles.field}>
          <label htmlFor={perHourId} className={styles.label}>
            {t('wl.notifications.editor.perHour')}
          </label>
          <input
            id={perHourId}
            type="number"
            min={1}
            step={1}
            inputMode="numeric"
            className={styles.narrowInput}
            aria-invalid={perHourIssue === undefined ? undefined : true}
            aria-describedby={describedBy('perHour', perHourHintId)}
            value={draft.perHour}
            onChange={(event) => {
              settings.setDraft({ ...draft, perHour: event.target.value });
            }}
          />
          <p id={perHourHintId} className={styles.hint}>
            {t('wl.notifications.editor.perHourHint')}
          </p>
          {perHourIssue === undefined ? null : (
            <p id={errorIdFor('perHour')} className={styles.error}>
              {perHourIssue}
            </p>
          )}
        </div>
        <p className={styles.hint}>{t('wl.notifications.editor.trigger')}</p>
      </fieldset>
      <DraftFooter status={settings} readOnly={readOnly} />
      {channel === null || readOnly ? null : (
        <div className={styles.toolbar}>
          <button
            type="button"
            className={styles.danger}
            onClick={() => {
              setConfirmDelete(true);
            }}
          >
            {t('wl.notifications.editor.delete')}
          </button>
        </div>
      )}
      <ConfirmDialog
        open={confirmDelete}
        title={t('wl.notifications.delete.title')}
        body={t('wl.notifications.delete.body')}
        confirmLabel={t('wl.notifications.delete.confirm')}
        cancelLabel={t('wl.notifications.delete.cancel')}
        busy={remove.isPending}
        onConfirm={removeChannel}
        onOpenChange={setConfirmDelete}
      />
    </div>
  );
}
