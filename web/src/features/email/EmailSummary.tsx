import type { ReactNode } from 'react';
import { useTranslation } from 'react-i18next';
import { formatAbsoluteTime, formatByteSize } from '../../components/format/format';
import { useTimeDisplay } from '../../prefs/display';
import { RelativeTime } from '../detail/RelativeTime';
import { MAX_FIELD_CHARS } from './constants';
import styles from './EmailDetail.module.css';
import { clampText, unfoldHeader } from './inert';
import { InertText } from './InertText';
import { findHeader } from './parseEmail';
import type { EmailView } from './types';

export interface EmailSummaryProps {
  view: EmailView;
}

interface FactProps {
  term: string;
  children: ReactNode;
}

interface AddressListProps {
  values: readonly string[];
}

function Fact({ term, children }: FactProps) {
  return (
    <div className={styles.fact}>
      <dt className={styles.factTerm}>{term}</dt>
      <dd className={styles.factValue}>{children}</dd>
    </div>
  );
}

function AddressList({ values }: AddressListProps) {
  return (
    <ul className={styles.plainList}>
      {values.map((value, index) => (
        <li key={index}>
          <InertText value={value} />
        </li>
      ))}
    </ul>
  );
}

function headerText(value: string | null): string | null {
  return value === null ? null : clampText(unfoldHeader(value), MAX_FIELD_CHARS);
}

export function EmailSummary({ view }: EmailSummaryProps) {
  const { t, i18n } = useTranslation();
  const locale = i18n.language;
  const timeDisplay = useTimeDisplay();
  const { capture, meta } = view;
  const subject = meta.subject ?? '';
  const replyTo = headerText(findHeader(capture.headers, 'reply-to'));
  const authentication = headerText(findHeader(capture.headers, 'authentication-results'));

  return (
    <div className={styles.stack}>
      <p className={styles.subject}>
        {subject === '' ? (
          <em className={styles.muted}>{t('wl.email.summary.noSubject')}</em>
        ) : (
          <InertText value={subject} />
        )}
      </p>
      <dl className={styles.facts}>
        <Fact term={t('wl.email.summary.from')}>
          {meta.from.length === 0 ? t('wl.email.none') : <AddressList values={meta.from} />}
        </Fact>
        <Fact term={t('wl.email.summary.to')}>
          {meta.to.length === 0 ? t('wl.email.none') : <AddressList values={meta.to} />}
        </Fact>
        {meta.cc.length === 0 ? null : (
          <Fact term={t('wl.email.summary.cc')}>
            <AddressList values={meta.cc} />
          </Fact>
        )}
        {replyTo === null ? null : (
          <Fact term={t('wl.email.summary.replyTo')}>
            <InertText value={replyTo} />
          </Fact>
        )}
        <Fact term={t('wl.email.summary.date')}>
          {meta.date === null ? t('wl.email.none') : <InertText value={formatAbsoluteTime(meta.date, locale)} />}
        </Fact>
        <Fact term={t('wl.email.summary.received')}>
          <time dateTime={capture.received_at}>
            {formatAbsoluteTime(capture.received_at, locale, timeDisplay === 'utc')}
          </time>
          {timeDisplay === 'relative' && (
            <>
              {' '}
              <span className={styles.muted}>
                <RelativeTime iso={capture.received_at} locale={locale} />
              </span>
            </>
          )}
        </Fact>
        <Fact term={t('wl.email.summary.size')}>{formatByteSize(capture.body_size, locale)}</Fact>
        <Fact term={t('wl.email.summary.via')}>
          {meta.helo === null || meta.helo === '' ? null : (
            <>
              <InertText value={meta.helo} />{' '}
            </>
          )}
          <code>
            <InertText value={capture.client_ip} />
          </code>
        </Fact>
        <Fact term={t('wl.email.summary.tls')}>
          {meta.tls ? t('wl.email.summary.tlsOn') : t('wl.email.summary.tlsOff')}
        </Fact>
        <Fact term={t('wl.email.summary.envelopeFrom')}>
          {meta.envelope_from === null || meta.envelope_from === '' ? (
            t('wl.email.none')
          ) : (
            <InertText value={meta.envelope_from} />
          )}
        </Fact>
        {meta.envelope_to.length === 0 ? null : (
          <Fact term={t('wl.email.summary.envelopeTo')}>
            <AddressList values={meta.envelope_to} />
          </Fact>
        )}
        {meta.tag === null || meta.tag === '' ? null : (
          <Fact term={t('wl.email.summary.tag')}>
            <InertText value={meta.tag} />
          </Fact>
        )}
        {meta.message_id === null || meta.message_id === '' ? null : (
          <Fact term={t('wl.email.summary.messageId')}>
            <code>
              <InertText value={meta.message_id} />
            </code>
          </Fact>
        )}
        {authentication === null ? null : (
          <Fact term={t('wl.email.summary.authentication')}>
            <InertText value={authentication} />
          </Fact>
        )}
      </dl>
    </div>
  );
}
