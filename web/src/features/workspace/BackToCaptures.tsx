import { Link } from '@tanstack/react-router';
import { useTranslation } from 'react-i18next';
import { rememberedSelection } from '../endpoint/selectionMemory';

export interface BackToCapturesProps {
  endpointId: string;
  className?: string;
}

export function BackToCaptures({ endpointId, className }: BackToCapturesProps) {
  const { t } = useTranslation();
  const captureId = rememberedSelection(endpointId);
  if (captureId === null) {
    return (
      <Link to="/e/$id" params={{ id: endpointId }} search className={className}>
        {t('wl.workspace.back')}
      </Link>
    );
  }
  return (
    <Link to="/e/$id/c/$captureId" params={{ id: endpointId, captureId }} search className={className}>
      {t('wl.workspace.back')}
    </Link>
  );
}
