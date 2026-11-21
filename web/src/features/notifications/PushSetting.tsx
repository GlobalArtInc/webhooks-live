import { useState } from 'react';
import { useTranslation } from 'react-i18next';
import { getPushPublicKey } from './notificationsApi';
import { currentPushEnvironment, pushSupportOf, subscribeThisBrowser, type PushOutcome } from './browserPush';
import styles from './ChannelEditor.module.css';

export interface PushSettingProps {
  signedIn: boolean;
}

type PushMessage = PushOutcome | 'unsupported' | 'failed';

export function PushSetting({ signedIn }: PushSettingProps) {
  const { t } = useTranslation();
  const [busy, setBusy] = useState(false);
  const [message, setMessage] = useState<PushMessage | null>(null);

  if (!signedIn) {
    return <p className={styles.hint}>{t('wl.notifications.push.signIn')}</p>;
  }
  if (pushSupportOf(currentPushEnvironment()) === 'unsupported') {
    return <p className={styles.hint}>{t('wl.notifications.push.unsupported')}</p>;
  }

  async function allow(): Promise<void> {
    setBusy(true);
    setMessage(null);
    try {
      const key = await getPushPublicKey();
      setMessage(await subscribeThisBrowser(key.public_key));
    } catch {
      setMessage('failed');
    } finally {
      setBusy(false);
    }
  }

  return (
    <div className={styles.field}>
      <p className={styles.hint}>{t('wl.notifications.push.intro')}</p>
      <div>
        <button type="button" className={styles.secondary} disabled={busy} onClick={() => void allow()}>
          {t('wl.notifications.push.allow')}
        </button>
      </div>
      {message === null ? null : <PushMessageLine message={message} />}
    </div>
  );
}

interface PushMessageLineProps {
  message: PushMessage;
}

function PushMessageLine({ message }: PushMessageLineProps) {
  const { t } = useTranslation();
  switch (message) {
    case 'subscribed':
      return (
        <p role="status" className={styles.hint}>
          {t('wl.notifications.push.subscribed')}
        </p>
      );
    case 'denied':
      return (
        <p role="alert" className={styles.error}>
          {t('wl.notifications.push.denied')}
        </p>
      );
    case 'needs-app-worker':
      return (
        <p role="alert" className={styles.error}>
          {t('wl.notifications.push.needsWorker')}
        </p>
      );
    case 'unsupported':
      return (
        <p role="alert" className={styles.error}>
          {t('wl.notifications.push.unsupported')}
        </p>
      );
    case 'failed':
      return (
        <p role="alert" className={styles.error}>
          {t('wl.notifications.push.failed')}
        </p>
      );
  }
}
