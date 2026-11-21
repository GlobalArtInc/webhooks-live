import { useTranslation } from 'react-i18next';
import { CopyButton } from '../../../components/CopyButton/CopyButton';
import { dnsNameOf, emailAddressOf, type AddressSource } from '../addresses/addressLines';
import { digSampleOf, emailSampleOf } from '../snippets';
import styles from './EmptySamples.module.css';

export interface EmptySamplesProps {
  endpoint: AddressSource;
}

interface SampleProps {
  label: string;
  command: string;
  copyLabel: string;
}

function Sample({ label, command, copyLabel }: SampleProps) {
  return (
    <div className={styles.sample}>
      <div className={styles.header}>
        <span className={styles.label}>{label}</span>
        <CopyButton value={command} label={copyLabel} />
      </div>
      <pre className={styles.code} tabIndex={0}>
        <code>{command}</code>
      </pre>
    </div>
  );
}

export function EmptySamples({ endpoint }: EmptySamplesProps) {
  const { t } = useTranslation();
  const email = emailAddressOf(endpoint);
  const dns = dnsNameOf(endpoint);
  if (email === null && dns === null) {
    return null;
  }
  return (
    <div className={styles.root}>
      {email === null ? null : (
        <Sample
          label={t('wl.kinds.samples.email')}
          command={emailSampleOf(email)}
          copyLabel={t('wl.kinds.samples.copyEmail')}
        />
      )}
      {dns === null ? null : (
        <Sample
          label={t('wl.kinds.samples.dig')}
          command={digSampleOf(dns)}
          copyLabel={t('wl.kinds.samples.copyDig')}
        />
      )}
    </div>
  );
}
