import { useTranslation } from 'react-i18next';
import { useStandalonePageTitle } from '../../../app/useDocumentTitle';
import { verifyEmailAddress } from '../notificationsApi';
import { ConfirmationScreen } from './ConfirmationScreen';

export function EmailVerifyScreen() {
  const { t } = useTranslation();
  useStandalonePageTitle(t('wl.title.emailVerify'));
  return (
    <ConfirmationScreen
      title={t('wl.emailpages.verify.title')}
      intro={t('wl.emailpages.verify.intro')}
      actionLabel={t('wl.emailpages.verify.action')}
      invalidText={t('wl.emailpages.verify.invalid')}
      failedText={t('wl.emailpages.verify.failed')}
      run={verifyEmailAddress}
      doneText={(address) => t('wl.emailpages.verify.done', { address: address.address })}
    />
  );
}
