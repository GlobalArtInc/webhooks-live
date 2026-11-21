import { useState } from 'react';
import { useTranslation } from 'react-i18next';
import { ApiError } from '../../api/client';
import { useEndpoint, useInstance } from '../../api/queries';
import type { SectionProps } from '../workspace/sections';
import { ChannelEditor } from './ChannelEditor';
import { ChannelTable, type TestOutcome } from './ChannelTable';
import { EmailAddresses } from './EmailAddresses';
import { kindsOffered } from './kinds';
import { useErrorText } from './issueText';
import { useChannels, useEmailAddresses, useTestChannel, useUpdateChannel } from './notificationsQueries';
import type { Channel } from './types';
import styles from './NotificationsSection.module.css';

const NEW_SELECTION = 'new';
const CHANNEL_LIMIT = 10;
const TEST_PER_MINUTE = 5;

export function NotificationsSection({ endpointId }: SectionProps) {
  const { t } = useTranslation();
  const instance = useInstance().data;
  const offered = instance?.features.notifications ?? [];
  const enabled = offered.length > 0;
  const signedIn = instance !== undefined && instance.user !== null;
  const kinds = kindsOffered(offered, signedIn);
  const channels = useChannels(endpointId, enabled);
  const addresses = useEmailAddresses(enabled && signedIn);
  const endpoint = useEndpoint(endpointId).data;
  const update = useUpdateChannel(endpointId);
  const test = useTestChannel(endpointId);
  const errorText = useErrorText();
  const [selection, setSelection] = useState<string | null>(null);
  const [outcomes, setOutcomes] = useState<Readonly<Record<string, TestOutcome>>>({});
  const heading = <h2 className={styles.heading}>{t('wl.workspace.section.notifications')}</h2>;

  if (instance === undefined) {
    return (
      <p role="status" className={styles.muted}>
        {t('wl.workspace.loading')}
      </p>
    );
  }

  if (!enabled || (channels.error instanceof ApiError && channels.error.code === 'feature_disabled')) {
    return (
      <div className={styles.page}>
        {heading}
        <p className={styles.muted}>{t('wl.notifications.unavailable')}</p>
      </div>
    );
  }

  if (channels.isError) {
    return (
      <div className={styles.page}>
        {heading}
        <p role="alert" className={styles.error}>
          {t('wl.notifications.error.load')}
        </p>
      </div>
    );
  }

  if (channels.data === undefined) {
    return (
      <div className={styles.page}>
        {heading}
        <p role="status" className={styles.muted}>
          {t('wl.workspace.loading')}
        </p>
      </div>
    );
  }

  const items = channels.data.items;
  const role = endpoint?.my_role ?? null;
  const readOnly = role !== 'owner' && role !== 'editor';
  const canCreate = !readOnly && kinds.length > 0 && items.length < CHANNEL_LIMIT;
  const selected = items.find((item) => item.id === selection) ?? null;
  const creating = selection === NEW_SELECTION;
  const emailAddresses = addresses.data?.items ?? [];

  function runTest(channelId: string): void {
    test.mutate(channelId, {
      onSuccess: (delivery) => {
        setOutcomes((current) => ({ ...current, [channelId]: { delivery, error: null, at: Date.now() } }));
      },
      onError: (error) => {
        setOutcomes((current) => ({ ...current, [channelId]: { delivery: null, error, at: Date.now() } }));
      },
    });
  }

  function toggle(channel: Channel): void {
    update.mutate({ channelId: channel.id, input: { enabled: !channel.enabled } });
  }

  return (
    <div className={styles.page}>
      {heading}
      <p className={styles.hint}>{t('wl.notifications.intro')}</p>
      <p className={styles.hint}>{t('wl.notifications.limits', { max: CHANNEL_LIMIT, perMinute: TEST_PER_MINUTE })}</p>
      <p className={styles.hint}>{t('wl.notifications.suppression')}</p>
      {readOnly ? <p className={styles.muted}>{t('wl.notifications.readOnly')}</p> : null}
      {update.isError ? (
        <p role="alert" className={styles.error}>
          {errorText(update.error)}
        </p>
      ) : null}
      <div className={styles.layout}>
        <div className={styles.listColumn}>
          <div className={styles.listHeader}>
            <button
              type="button"
              className={styles.add}
              disabled={!canCreate || creating}
              onClick={() => {
                setSelection(NEW_SELECTION);
              }}
            >
              {t('wl.notifications.add')}
            </button>
          </div>
          {items.length === 0 && !creating ? <p className={styles.muted}>{t('wl.notifications.empty')}</p> : null}
          {items.length === 0 ? null : (
            <ChannelTable
              channels={items}
              emailAddresses={emailAddresses}
              selectedId={selection}
              readOnly={readOnly}
              outcomes={outcomes}
              onEdit={setSelection}
              onTest={runTest}
              onToggle={toggle}
            />
          )}
          {signedIn ? <EmailAddresses addresses={emailAddresses} /> : null}
        </div>
        <div className={styles.editorColumn}>
          {creating ? (
            <ChannelEditor
              key={NEW_SELECTION}
              endpointId={endpointId}
              channel={null}
              kinds={kinds}
              readOnly={readOnly}
              emailAddresses={emailAddresses}
              signedIn={signedIn}
              onCreated={setSelection}
              onDeleted={() => {
                setSelection(null);
              }}
            />
          ) : null}
          {selected === null ? null : (
            <ChannelEditor
              key={selected.id}
              endpointId={endpointId}
              channel={selected}
              kinds={kinds}
              readOnly={readOnly}
              emailAddresses={emailAddresses}
              signedIn={signedIn}
              onCreated={setSelection}
              onDeleted={() => {
                setSelection(null);
              }}
            />
          )}
        </div>
      </div>
    </div>
  );
}
