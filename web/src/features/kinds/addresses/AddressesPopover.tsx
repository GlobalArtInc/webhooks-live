import * as Popover from '@radix-ui/react-popover';
import { useTranslation } from 'react-i18next';
import { CopyButton } from '../../../components/CopyButton/CopyButton';
import { digSampleOf } from '../snippets';
import { addressCountOf, dnsNameOf, emailAddressOf, type AddressSource } from './addressLines';
import styles from './AddressesPopover.module.css';

export interface AddressesPopoverProps {
  endpoint: AddressSource;
  curl: string;
}

export function AddressesPopover({ endpoint, curl }: AddressesPopoverProps) {
  const { t } = useTranslation();
  const email = emailAddressOf(endpoint);
  const dns = dnsNameOf(endpoint);
  return (
    <Popover.Root>
      <Popover.Trigger asChild>
        <button type="button" className={styles.trigger}>
          {t('wl.kinds.addresses.trigger', { count: addressCountOf(endpoint) })}
        </button>
      </Popover.Trigger>
      <Popover.Portal>
        <Popover.Content
          className={styles.content}
          align="end"
          sideOffset={6}
          aria-label={t('wl.kinds.addresses.title')}
        >
          <ul className={styles.list}>
            <li className={styles.row}>
              <span className={styles.kind}>{t('wl.kinds.addresses.http')}</span>
              <code className={styles.value}>{endpoint.url}</code>
              <span className={styles.actions}>
                <CopyButton value={endpoint.url} label={t('wl.kinds.addresses.copyUrl')} />
                <CopyButton value={curl} label={t('wl.kinds.addresses.copyCurl')} />
              </span>
            </li>
            {email === null ? null : (
              <li className={styles.row}>
                <span className={styles.kind}>{t('wl.kinds.addresses.email')}</span>
                <code className={styles.value}>{email}</code>
                <span className={styles.actions}>
                  <CopyButton value={email} label={t('wl.kinds.addresses.copyEmail')} />
                </span>
              </li>
            )}
            {dns === null ? null : (
              <li className={styles.row}>
                <span className={styles.kind}>{t('wl.kinds.addresses.dns')}</span>
                <code className={styles.value}>{dns}</code>
                <span className={styles.actions}>
                  <CopyButton value={dns} label={t('wl.kinds.addresses.copyDns')} />
                  <CopyButton value={digSampleOf(dns)} label={t('wl.kinds.addresses.copyDig')} />
                </span>
                <p className={styles.note}>{t('wl.kinds.addresses.alsoCaught', { example: `any-label.${dns}` })}</p>
              </li>
            )}
          </ul>
        </Popover.Content>
      </Popover.Portal>
    </Popover.Root>
  );
}
