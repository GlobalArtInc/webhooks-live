import { useState } from 'react';
import { useTranslation } from 'react-i18next';
import { CopyAction } from './CopyAction';
import { formatCurl, formatHttp, type OperationSample } from './samples';
import styles from './SamplePanel.module.css';

export interface SamplePanelProps {
  sample: OperationSample;
  operationLabel: string;
}

type SampleFormat = 'curl' | 'http';

export function SamplePanel({ sample, operationLabel }: SamplePanelProps) {
  const { t } = useTranslation();
  const [format, setFormat] = useState<SampleFormat>('curl');
  const text = format === 'curl' ? formatCurl(sample) : formatHttp(sample);
  const formatName = format === 'curl' ? t('wl.apireference.sample.curl') : t('wl.apireference.sample.http');
  return (
    <div className={styles.panel}>
      <div className={styles.bar}>
        <div role="group" aria-label={t('wl.apireference.sample.formats')} className={styles.formats}>
          <button
            type="button"
            className={styles.format}
            aria-pressed={format === 'curl'}
            onClick={() => setFormat('curl')}
          >
            {t('wl.apireference.sample.curl')}
          </button>
          <button
            type="button"
            className={styles.format}
            aria-pressed={format === 'http'}
            onClick={() => setFormat('http')}
          >
            {t('wl.apireference.sample.http')}
          </button>
        </div>
        <CopyAction
          value={text}
          label={t('wl.apireference.sample.copyLabel', { format: formatName, operation: operationLabel })}
        />
      </div>
      <pre className={styles.code} tabIndex={0}>
        {text}
      </pre>
    </div>
  );
}
