import { useEffect, useState } from 'react';
import { useTranslation } from 'react-i18next';
import { useInstance } from '../../api/queries';
import { copyText } from '../../components/CopyButton/clipboard';
import { Copy } from '../../components/icons';
import { captureUrlOf } from '../../features/endpoints/format';
import { byRecentlyOpened } from '../../features/endpoints/storage';
import { useMyEndpoints } from '../../features/endpoints/store';
import { useCachedEndpointInfos } from '../../features/endpoints/titleCache';
import styles from './CurlExample.module.css';

const PLACEHOLDER_ID = 'YOUR_ENDPOINT_ID';
const SAMPLE_PATH = 'hello';
const SAMPLE_BODY = '{"hello":"world"}';
const FEEDBACK_MS = 1500;

type CopyState = 'idle' | 'copied' | 'failed';

export function curlCommand(captureUrl: string): string {
  return `curl -X POST ${captureUrl}/${SAMPLE_PATH} -d '${SAMPLE_BODY}'`;
}

export function CurlExample() {
  const { t } = useTranslation();
  const latest = byRecentlyOpened(useMyEndpoints())[0];
  const [info] = useCachedEndpointInfos(latest === undefined ? [] : [latest.id]);
  const publicUrl = useInstance().data?.public_url ?? null;
  const [state, setState] = useState<CopyState>('idle');
  const realUrl = latest === undefined ? null : captureUrlOf(latest.id, info ?? null, publicUrl);
  const captureUrl = realUrl ?? `${window.location.origin}/${PLACEHOLDER_ID}`;
  const command = curlCommand(captureUrl);

  useEffect(() => {
    if (state === 'idle') {
      return undefined;
    }
    const timer = window.setTimeout(() => {
      setState('idle');
    }, FEEDBACK_MS);
    return () => {
      window.clearTimeout(timer);
    };
  }, [state]);

  const buttonText =
    state === 'copied'
      ? t('wl.detail.copy.copied')
      : state === 'failed'
        ? t('wl.detail.copy.failed')
        : t('wl.detail.copy.idle');

  return (
    <div className={styles.block}>
      <span className={styles.label}>{realUrl === null ? t('wl.home.curl.label') : t('wl.home.curl.labelLatest')}</span>
      <div className={styles.row}>
        <code className={styles.code}>{command}</code>
        <button
          type="button"
          className={styles.copy}
          aria-label={t('wl.home.curl.copyLabel')}
          data-state={state}
          onClick={() => {
            void copyText(command).then((copied) => {
              setState(copied ? 'copied' : 'failed');
            });
          }}
        >
          <Copy size={18} />
          {buttonText}
        </button>
      </div>
      <span role="status" className={styles.status}>
        {state === 'idle' ? '' : buttonText}
      </span>
    </div>
  );
}
