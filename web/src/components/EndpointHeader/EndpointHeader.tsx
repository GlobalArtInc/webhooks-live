import { Link } from '@tanstack/react-router';
import type { ReactNode } from 'react';
import { useTranslation } from 'react-i18next';
import { formatRelativeTime } from '../format/format';
import { CopyButton } from '../CopyButton/CopyButton';
import { MiddleTruncate } from '../MiddleTruncate/MiddleTruncate';
import { AddressesPopover } from '../../features/kinds/addresses/AddressesPopover';
import type { AddressSource } from '../../features/kinds/addresses/addressLines';
import { HeaderMenu } from './HeaderMenu';
import styles from './EndpointHeader.module.css';

const NEAR_CAPACITY_RATIO = 0.9;
const NARROW_TAIL_LENGTH = 6;
const SCHEME_PATTERN = /^[a-z][a-z0-9+.-]*:\/\//i;

interface UrlParts {
  prefix: string;
  id: string;
}

function splitCaptureUrl(url: string): UrlParts {
  const bare = url.replace(SCHEME_PATTERN, '');
  const cut = bare.lastIndexOf('/') + 1;
  return { prefix: bare.slice(0, cut), id: bare.slice(cut) };
}

export interface EndpointHeaderProps {
  endpointId: string;
  captureUrl: string;
  curl: string;
  count: number;
  capacity: number;
  expiresAt: string;
  now: number;
  narrow: boolean;
  status?: ReactNode;
  addresses?: AddressSource;
  onDeleteAll: () => void;
}

type CapacityState = 'normal' | 'near' | 'full';

function capacityStateOf(count: number, capacity: number): CapacityState {
  if (count >= capacity) {
    return 'full';
  }
  return count >= capacity * NEAR_CAPACITY_RATIO ? 'near' : 'normal';
}

export function EndpointHeader({
  endpointId,
  captureUrl,
  curl,
  count,
  capacity,
  expiresAt,
  now,
  narrow,
  status,
  addresses,
  onDeleteAll,
}: EndpointHeaderProps) {
  const { t, i18n } = useTranslation();
  const locale = i18n.language;
  const capacityState = capacityStateOf(count, capacity);
  const urlParts = splitCaptureUrl(captureUrl);
  const expiry = formatRelativeTime(expiresAt, now, locale);
  const addressSource =
    addresses !== undefined && (addresses.email !== undefined || addresses.dns !== undefined) ? addresses : undefined;

  return (
    <div className={styles.root}>
      <div className={styles.urlGroup}>
        <span className={styles.label}>{t('wl.endpoint.header.captureUrl')}</span>
        <code className={styles.url}>
          {narrow ? (
            <MiddleTruncate prefix={urlParts.prefix} text={urlParts.id} tailLength={NARROW_TAIL_LENGTH} />
          ) : (
            <MiddleTruncate text={captureUrl} />
          )}
        </code>
        <CopyButton
          value={captureUrl}
          label={t('wl.endpoint.header.copyUrl')}
          idleText={t('wl.endpoint.header.copyUrl')}
        />
        {addressSource === undefined ? null : <AddressesPopover endpoint={addressSource} curl={curl} />}
      </div>
      <div className={styles.meta}>
        <span className={styles.count}>{t('wl.endpoint.header.count', { count, capacity })}</span>
        {capacityState === 'normal' ? null : (
          <span className={styles.capacity} data-state={capacityState}>
            {capacityState === 'full' ? t('wl.endpoint.header.full') : t('wl.endpoint.header.nearCapacity')}
          </span>
        )}
        <span className={styles.expiry}>{t('wl.endpoint.header.expires', { when: expiry })}</span>
        {status}
      </div>
      <div className={styles.actions}>
        {narrow ? (
          <HeaderMenu endpointId={endpointId} curl={curl} disableDeleteAll={count === 0} onDeleteAll={onDeleteAll} />
        ) : (
          <>
            <CopyButton
              value={curl}
              label={t('wl.endpoint.header.copyCurl')}
              idleText={t('wl.endpoint.header.copyCurl')}
            />
            <Link to="/e/$id/settings" params={{ id: endpointId }} className={styles.button}>
              {t('wl.endpoint.header.settings')}
            </Link>
            <Link to="/e/$id/forwarding" params={{ id: endpointId }} className={styles.button}>
              {t('wl.endpoint.header.forwarding')}
            </Link>
            <button type="button" className={styles.danger} disabled={count === 0} onClick={onDeleteAll}>
              {t('wl.endpoint.header.deleteAll')}
            </button>
          </>
        )}
      </div>
    </div>
  );
}
