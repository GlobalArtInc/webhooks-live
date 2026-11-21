import { useTranslation } from 'react-i18next';
import type { Channel, ChannelKind, Delivery, EmailAddress } from './types';
import { useErrorText } from './issueText';
import styles from './ChannelTable.module.css';

export interface TestOutcome {
  delivery: Delivery | null;
  error: unknown;
  at: number;
}

export interface ChannelTableProps {
  channels: readonly Channel[];
  emailAddresses: readonly EmailAddress[];
  selectedId: string | null;
  readOnly: boolean;
  outcomes: Readonly<Record<string, TestOutcome>>;
  onEdit: (channelId: string) => void;
  onTest: (channelId: string) => void;
  onToggle: (channel: Channel) => void;
}

interface RowProps {
  channel: Channel;
  emailAddresses: readonly EmailAddress[];
  selected: boolean;
  readOnly: boolean;
  outcome: TestOutcome | undefined;
  onEdit: (channelId: string) => void;
  onTest: (channelId: string) => void;
  onToggle: (channel: Channel) => void;
}

function clockOf(at: number): string {
  return new Intl.DateTimeFormat(undefined, { hour: '2-digit', minute: '2-digit' }).format(at);
}

interface TargetTextProps {
  channel: Channel;
  emailAddresses: readonly EmailAddress[];
}

function TargetText({ channel, emailAddresses }: TargetTextProps) {
  const { t } = useTranslation();
  switch (channel.kind) {
    case 'webhook':
    case 'slack':
    case 'discord':
      return <>{channel.config.url ?? ''}</>;
    case 'telegram':
      return (
        <>
          {t('wl.notifications.row.telegramTarget', {
            chat: channel.config.chat_id ?? '',
            secret: channel.config.token_secret ?? '',
          })}
        </>
      );
    case 'email': {
      const address = emailAddresses.find((item) => item.id === channel.config.address_id);
      return <>{address?.address ?? t('wl.notifications.row.unknownAddress')}</>;
    }
    case 'push':
      return <>{t('wl.notifications.row.pushTarget')}</>;
  }
}

interface KindNameProps {
  kind: ChannelKind;
}

export function KindName({ kind }: KindNameProps) {
  const { t } = useTranslation();
  switch (kind) {
    case 'webhook':
      return <>{t('wl.notifications.kind.webhook')}</>;
    case 'slack':
      return <>{t('wl.notifications.kind.slack')}</>;
    case 'discord':
      return <>{t('wl.notifications.kind.discord')}</>;
    case 'telegram':
      return <>{t('wl.notifications.kind.telegram')}</>;
    case 'email':
      return <>{t('wl.notifications.kind.email')}</>;
    case 'push':
      return <>{t('wl.notifications.kind.push')}</>;
  }
}

interface OutcomeLineProps {
  outcome: TestOutcome;
}

function OutcomeLine({ outcome }: OutcomeLineProps) {
  const { t } = useTranslation();
  const errorText = useErrorText();
  const time = clockOf(outcome.at);
  if (outcome.error !== null && outcome.error !== undefined) {
    return (
      <p role="alert" className={styles.failed}>
        {t('wl.notifications.test.errorAt', { time, detail: errorText(outcome.error) })}
      </p>
    );
  }
  const delivery = outcome.delivery;
  if (delivery === null) {
    return null;
  }
  if (delivery.state === 'succeeded') {
    return (
      <p role="status" className={styles.ok}>
        {t('wl.notifications.test.succeeded', { time })}
      </p>
    );
  }
  if (delivery.state === 'failed') {
    const detail = [delivery.status === null ? '' : String(delivery.status), delivery.error ?? '']
      .filter((part) => part !== '')
      .join(' ');
    return (
      <p role="alert" className={styles.failed}>
        {t('wl.notifications.test.failed', {
          time,
          detail: detail === '' ? t('wl.notifications.test.noDetail') : detail,
        })}
      </p>
    );
  }
  if (delivery.state === 'suppressed') {
    return (
      <p role="status" className={styles.muted}>
        {t('wl.notifications.test.suppressed', { time })}
      </p>
    );
  }
  return (
    <p role="status" className={styles.muted}>
      {t('wl.notifications.test.queued', { time })}
    </p>
  );
}

function ChannelRow({ channel, emailAddresses, selected, readOnly, outcome, onEdit, onTest, onToggle }: RowProps) {
  const { t } = useTranslation();
  return (
    <li className={selected ? `${styles.row} ${styles.selected}` : styles.row}>
      <div className={styles.main}>
        <p className={styles.name}>{channel.name}</p>
        <p className={styles.meta}>
          <span className={styles.kind}>
            <KindName kind={channel.kind} />
          </span>
          <span>{t('wl.notifications.row.everyCapture')}</span>
          <span>{t('wl.notifications.row.perHour', { count: channel.per_hour })}</span>
        </p>
        <p className={styles.target}>
          <TargetText channel={channel} emailAddresses={emailAddresses} />
        </p>
        {outcome === undefined ? null : <OutcomeLine outcome={outcome} />}
      </div>
      <div className={styles.actions}>
        <button
          type="button"
          role="switch"
          aria-checked={channel.enabled}
          aria-label={t('wl.notifications.row.switch', { name: channel.name })}
          className={channel.enabled ? `${styles.switch} ${styles.on}` : styles.switch}
          disabled={readOnly}
          onClick={() => {
            onToggle(channel);
          }}
        >
          <span className={styles.knob} aria-hidden="true" />
          <span>{channel.enabled ? t('wl.notifications.row.on') : t('wl.notifications.row.off')}</span>
        </button>
        <button
          type="button"
          className={styles.button}
          disabled={readOnly}
          aria-label={t('wl.notifications.row.testFor', { name: channel.name })}
          onClick={() => {
            onTest(channel.id);
          }}
        >
          {t('wl.notifications.row.test')}
        </button>
        <button
          type="button"
          className={styles.button}
          aria-pressed={selected}
          aria-label={t('wl.notifications.row.editFor', { name: channel.name })}
          onClick={() => {
            onEdit(channel.id);
          }}
        >
          {t('wl.notifications.row.edit')}
        </button>
      </div>
    </li>
  );
}

export function ChannelTable({
  channels,
  emailAddresses,
  selectedId,
  readOnly,
  outcomes,
  onEdit,
  onTest,
  onToggle,
}: ChannelTableProps) {
  const { t } = useTranslation();
  return (
    <ul className={styles.list} aria-label={t('wl.notifications.list.label')}>
      {channels.map((channel) => (
        <ChannelRow
          key={channel.id}
          channel={channel}
          emailAddresses={emailAddresses}
          selected={channel.id === selectedId}
          readOnly={readOnly}
          outcome={outcomes[channel.id]}
          onEdit={onEdit}
          onTest={onTest}
          onToggle={onToggle}
        />
      ))}
    </ul>
  );
}
