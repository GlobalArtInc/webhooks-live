import { useTranslation } from 'react-i18next';
import { disableNotifications } from '../notificationsApi';
import { ConfirmationScreen } from './ConfirmationScreen';

export function DisableNotificationsScreen() {
  const { t } = useTranslation();
  return (
    <ConfirmationScreen
      title={t('wl.emailpages.disable.title')}
      intro={t('wl.emailpages.disable.intro')}
      actionLabel={t('wl.emailpages.disable.action')}
      invalidText={t('wl.emailpages.disable.invalid')}
      failedText={t('wl.emailpages.disable.failed')}
      run={disableNotifications}
      doneText={() => t('wl.emailpages.disable.done')}
    />
  );
}
