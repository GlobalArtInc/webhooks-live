import { useTranslation } from 'react-i18next';
import { forgetEndpoint } from './store';
import styles from './RemoveFromMyEndpoints.module.css';

export interface RemoveFromMyEndpointsProps {
  endpointId: string;
  onRemoved?: () => void;
}

export function RemoveFromMyEndpoints({ endpointId, onRemoved }: RemoveFromMyEndpointsProps) {
  const { t } = useTranslation();

  function remove() {
    forgetEndpoint(endpointId);
    onRemoved?.();
  }

  return (
    <button type="button" className={styles.button} onClick={remove}>
      {t('wl.home.remove.button')}
    </button>
  );
}
