import { useId, type ReactNode } from 'react';
import { useTranslation } from 'react-i18next';
import type { Capture } from '../../../api/types';
import { formatAbsoluteTime } from '../../../components/format/format';
import styles from './DnsDetail.module.css';
import { dnsMetaOf, dnsTypeNumber, labelOf, type DnsMeta } from './dnsMeta';

export interface DnsDetailProps {
  capture: Capture;
  dnsName?: string | null;
}

interface FactProps {
  term: string;
  children: ReactNode;
}

function Fact({ term, children }: FactProps) {
  return (
    <div className={styles.fact}>
      <dt className={styles.term}>{term}</dt>
      <dd className={styles.value}>{children}</dd>
    </div>
  );
}

export function DnsDetail({ capture, dnsName }: DnsDetailProps) {
  const { t, i18n } = useTranslation();
  const summaryId = useId();
  const queryId = useId();
  const dns = dnsMetaOf(capture.meta);
  if (dns === null) {
    return (
      <p role="status" className={styles.unreadable}>
        {t('wl.kinds.dns.unreadable')}
      </p>
    );
  }
  const label = labelOf(dns.name, dnsName);
  const transport =
    dns.transport === null
      ? t('wl.kinds.dns.none')
      : dns.transport === 'udp'
        ? t('wl.kinds.dns.transport.udp')
        : t('wl.kinds.dns.transport.tcp');
  const flags: string[] = [];
  if (dns.flags.rd) {
    flags.push(t('wl.kinds.dns.flag.rd'));
  }
  if (dns.flags.cd) {
    flags.push(t('wl.kinds.dns.flag.cd'));
  }
  if (dns.flags.do) {
    flags.push(t('wl.kinds.dns.flag.do'));
  }
  return (
    <div className={styles.root} role="group" aria-label={t('wl.kinds.dns.title')}>
      <section aria-labelledby={summaryId} className={styles.section}>
        <h3 id={summaryId} className={styles.heading}>
          {t('wl.kinds.dns.section.summary')}
        </h3>
        <dl className={styles.facts}>
          <Fact term={t('wl.kinds.dns.field.name')}>
            <code>{dns.name}</code>
          </Fact>
          <Fact term={t('wl.kinds.dns.field.type')}>{typeText(dns)}</Fact>
          <Fact term={t('wl.kinds.dns.field.class')}>
            <code>{dns.class}</code>
          </Fact>
          {label === null ? null : (
            <Fact term={t('wl.kinds.dns.field.label')}>
              <code>{label}</code>
            </Fact>
          )}
          <Fact term={t('wl.kinds.dns.field.askedBy')}>
            <code>{capture.client_ip}</code>
          </Fact>
          <Fact term={t('wl.kinds.dns.field.transport')}>{transport}</Fact>
          <Fact term={t('wl.kinds.dns.field.received')}>
            <time dateTime={capture.received_at}>{formatAbsoluteTime(capture.received_at, i18n.language)}</time>
          </Fact>
        </dl>
      </section>
      <section aria-labelledby={queryId} className={styles.section}>
        <h3 id={queryId} className={styles.heading}>
          {t('wl.kinds.dns.section.query')}
        </h3>
        <dl className={styles.facts}>
          <Fact term={t('wl.kinds.dns.field.flags')}>
            {flags.length === 0 ? t('wl.kinds.dns.noFlags') : flags.join('; ')}
          </Fact>
          <Fact term={t('wl.kinds.dns.field.ednsSize')}>
            {dns.ednsUdpSize === null ? t('wl.kinds.dns.none') : String(dns.ednsUdpSize)}
          </Fact>
          <Fact term={t('wl.kinds.dns.field.subnet')}>
            {dns.clientSubnet === null ? t('wl.kinds.dns.none') : <code>{dns.clientSubnet}</code>}
          </Fact>
          <Fact term={t('wl.kinds.dns.field.messageId')}>
            {dns.dnsId === null ? t('wl.kinds.dns.none') : String(dns.dnsId)}
          </Fact>
        </dl>
      </section>
    </div>
  );
}

function typeText(dns: DnsMeta): string {
  const number = dnsTypeNumber(dns.type);
  return number === undefined ? dns.type : `${dns.type} (${number})`;
}
