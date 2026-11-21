import { useParams } from '@tanstack/react-router';
import { useTranslation } from 'react-i18next';
import { SideSheet } from '../../components/SideSheet/SideSheet';
import { useCloseSheet } from '../../features/endpoint/useCloseSheet';

export function ForwardingSheetPlaceholder() {
  const { t } = useTranslation();
  const { id } = useParams({ from: '/shell/e/$id/forwarding' });
  const close = useCloseSheet(id);
  return (
    <SideSheet title={t('wl.endpoint.sheet.forwarding.title')} onClose={close}>
      <p>{t('wl.endpoint.sheet.placeholder')}</p>
    </SideSheet>
  );
}
