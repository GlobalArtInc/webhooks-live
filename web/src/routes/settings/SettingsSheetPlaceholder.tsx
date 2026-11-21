import { useParams } from '@tanstack/react-router';
import { useTranslation } from 'react-i18next';
import { SideSheet } from '../../components/SideSheet/SideSheet';
import { useCloseSheet } from '../../features/endpoint/useCloseSheet';

export function SettingsSheetPlaceholder() {
  const { t } = useTranslation();
  const { id } = useParams({ from: '/shell/e/$id/settings' });
  const close = useCloseSheet(id);
  return (
    <SideSheet title={t('wl.endpoint.sheet.settings.title')} onClose={close}>
      <p>{t('wl.endpoint.sheet.placeholder')}</p>
    </SideSheet>
  );
}
