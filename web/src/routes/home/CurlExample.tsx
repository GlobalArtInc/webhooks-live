import { useTranslation } from 'react-i18next';
import { useInstance } from '../../api/queries';
import { CopyButton } from '../../components/CopyButton/CopyButton';
import { captureUrlOf } from '../../features/endpoints/format';
import { byRecentlyOpened } from '../../features/endpoints/storage';
import { useMyEndpoints } from '../../features/endpoints/store';
import { useCachedEndpointInfos } from '../../features/endpoints/titleCache';
import styles from './CurlExample.module.css';

const PLACEHOLDER_ID = 'YOUR_ENDPOINT_ID';
const SAMPLE_PATH = 'hello';
const SAMPLE_BODY = '{"hello":"world"}';

export function curlCommand(captureUrl: string): string {
  return `curl -X POST ${captureUrl}/${SAMPLE_PATH} -d '${SAMPLE_BODY}'`;
}

export function CurlExample() {
  const { t } = useTranslation();
  const latest = byRecentlyOpened(useMyEndpoints())[0];
  const [info] = useCachedEndpointInfos(latest === undefined ? [] : [latest.id]);
  const publicUrl = useInstance().data?.public_url ?? null;
  const realUrl = latest === undefined ? null : captureUrlOf(latest.id, info ?? null, publicUrl);
  const base = (publicUrl ?? window.location.origin).replace(/\/+$/, '');
  const captureUrl = realUrl ?? `${base}/${PLACEHOLDER_ID}`;
  const command = curlCommand(captureUrl);

  return (
    <div className={styles.block}>
      <span className={styles.label}>{realUrl === null ? t('wl.home.curl.label') : t('wl.home.curl.labelLatest')}</span>
      <div className={styles.row}>
        <code className={styles.code}>{command}</code>
        <CopyButton value={command} label={t('wl.home.curl.copyLabel')} size="large" />
      </div>
    </div>
  );
}
