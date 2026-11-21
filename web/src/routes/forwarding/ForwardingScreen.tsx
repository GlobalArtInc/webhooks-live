import { useTranslation } from 'react-i18next';
import { ScreenPlaceholder } from '../../components/ScreenPlaceholder/ScreenPlaceholder';

export function ForwardingScreen() {
  const { t } = useTranslation();
  return <ScreenPlaceholder title={t('wl.screen.forwarding')} />;
}
