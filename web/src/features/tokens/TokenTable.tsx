import type { TFunction } from 'i18next';
import { useTranslation } from 'react-i18next';
import type { ApiToken } from '../../api/tokens';
import { formatRelativeTime } from '../../components/format/format';
import { useLanguage } from '../../i18n/provider';
import { useViewportLayout } from '../endpoint/useViewport';
import { useNow } from '../endpoints/useNow';
import { shortEndpointId } from '../endpoints/format';
import { scopeLabel } from './scopeLabels';
import { tokenState, type TokenState } from './tokenState';
import { formatDate } from './time';
import styles from './TokenTable.module.css';

export interface TokenTableProps {
  tokens: readonly ApiToken[];
  onRevoke: (token: ApiToken) => void;
}

interface RowProps {
  token: ApiToken;
  now: number;
  locale: string;
  onRevoke: (token: ApiToken) => void;
}

interface StateBadgeProps {
  state: TokenState;
}

interface ScopeListProps {
  scopes: readonly string[];
}

interface EndpointsCellProps {
  ids: readonly string[] | null;
}

interface ExpiryTextProps {
  token: ApiToken;
  state: TokenState;
  now: number;
  locale: string;
}

interface LastUsedProps {
  token: ApiToken;
  now: number;
  locale: string;
}

interface RevokeButtonProps {
  token: ApiToken;
  onRevoke: (token: ApiToken) => void;
}

function stateLabel(t: TFunction, state: TokenState): string {
  switch (state) {
    case 'active':
      return t('wl.tokens.state.active');
    case 'expired':
      return t('wl.tokens.state.expired');
    case 'revoked':
      return t('wl.tokens.state.revoked');
  }
}

function StateBadge({ state }: StateBadgeProps) {
  const { t } = useTranslation();
  return (
    <span className={styles.badge} data-state={state}>
      {stateLabel(t, state)}
    </span>
  );
}

function ScopeList({ scopes }: ScopeListProps) {
  const { t } = useTranslation();
  return (
    <ul className={styles.scopes}>
      {scopes.map((scope) => (
        <li key={scope}>{scopeLabel(t, scope)}</li>
      ))}
    </ul>
  );
}

function EndpointsCell({ ids }: EndpointsCellProps) {
  const { t } = useTranslation();
  if (ids === null) {
    return <span>{t('wl.tokens.endpoints.all')}</span>;
  }
  return (
    <details className={styles.details}>
      <summary>{t('wl.tokens.endpoints.some', { count: ids.length })}</summary>
      <ul className={styles.ids} aria-label={t('wl.tokens.endpoints.ids')}>
        {ids.map((id) => (
          <li key={id}>
            <code>{shortEndpointId(id)}</code>
          </li>
        ))}
      </ul>
    </details>
  );
}

function expiryText(token: ApiToken, state: TokenState, now: number, locale: string): string {
  return state === 'revoked' && token.revoked_at !== null
    ? formatDate(token.revoked_at, locale)
    : formatRelativeTime(token.expires_at, now, locale);
}

function ExpiryText({ token, state, now, locale }: ExpiryTextProps) {
  const { t } = useTranslation();
  const when = expiryText(token, state, now, locale);
  if (state === 'revoked') {
    return <span>{t('wl.tokens.expires.revoked', { when })}</span>;
  }
  if (state === 'expired') {
    return <span>{t('wl.tokens.expires.expired', { when })}</span>;
  }
  return <span>{t('wl.tokens.expires.active', { when })}</span>;
}

function LastUsed({ token, now, locale }: LastUsedProps) {
  const { t } = useTranslation();
  if (token.last_used_at === null) {
    return <span>{t('wl.tokens.lastUsed.never')}</span>;
  }
  return <span>{formatRelativeTime(token.last_used_at, now, locale)}</span>;
}

function RevokeButton({ token, onRevoke }: RevokeButtonProps) {
  const { t } = useTranslation();
  return (
    <button
      type="button"
      className={styles.revoke}
      aria-label={t('wl.tokens.revoke.label', { name: token.name })}
      onClick={() => onRevoke(token)}
    >
      {t('wl.tokens.revoke.action')}
    </button>
  );
}

function TableRow({ token, now, locale, onRevoke }: RowProps) {
  const { t } = useTranslation();
  const state = tokenState(token, now);
  return (
    <tr className={styles.row} data-state={state}>
      <th scope="row" className={styles.nameCell}>
        <span className={styles.name}>{token.name}</span>
        <code className={styles.prefix}>{t('wl.tokens.prefix', { prefix: token.prefix })}</code>
      </th>
      <td className={styles.cell}>
        <ScopeList scopes={token.scopes} />
      </td>
      <td className={styles.cell}>
        <EndpointsCell ids={token.endpoint_ids} />
      </td>
      <td className={styles.cell}>{formatDate(token.created_at, locale)}</td>
      <td className={styles.cell}>
        <LastUsed token={token} now={now} locale={locale} />
      </td>
      <td className={styles.cell}>
        <ExpiryText token={token} state={state} now={now} locale={locale} />
      </td>
      <td className={styles.cell}>
        <StateBadge state={state} />
      </td>
      <td className={styles.cell}>{state === 'active' && <RevokeButton token={token} onRevoke={onRevoke} />}</td>
    </tr>
  );
}

function TokenCard({ token, now, locale, onRevoke }: RowProps) {
  const { t } = useTranslation();
  const state = tokenState(token, now);
  return (
    <li className={styles.card} data-state={state}>
      <div className={styles.cardHead}>
        <div className={styles.cardTitle}>
          <span className={styles.name}>{token.name}</span>
          <code className={styles.prefix}>{t('wl.tokens.prefix', { prefix: token.prefix })}</code>
        </div>
        <StateBadge state={state} />
      </div>
      <ScopeList scopes={token.scopes} />
      <p className={styles.cardLine}>
        <EndpointsCell ids={token.endpoint_ids} />
      </p>
      <p className={styles.cardLine}>{t('wl.tokens.created', { date: formatDate(token.created_at, locale) })}</p>
      <p className={styles.cardLine}>
        <ExpiryText token={token} state={state} now={now} locale={locale} />
      </p>
      <p className={styles.cardLine}>
        {token.last_used_at === null
          ? t('wl.tokens.lastUsed.never')
          : t('wl.tokens.lastUsed.value', { when: formatRelativeTime(token.last_used_at, now, locale) })}
      </p>
      {state === 'active' && (
        <div className={styles.cardActions}>
          <RevokeButton token={token} onRevoke={onRevoke} />
        </div>
      )}
    </li>
  );
}

export function TokenTable({ tokens, onRevoke }: TokenTableProps) {
  const { t } = useTranslation();
  const { language } = useLanguage();
  const layout = useViewportLayout();
  const now = useNow();

  if (layout === 'narrow') {
    return (
      <ul className={styles.cards} aria-label={t('wl.tokens.table.label')}>
        {tokens.map((token) => (
          <TokenCard key={token.id} token={token} now={now} locale={language} onRevoke={onRevoke} />
        ))}
      </ul>
    );
  }

  return (
    <div className={styles.scroller} role="region" tabIndex={0} aria-label={t('wl.tokens.table.scroll')}>
      <table className={styles.table}>
        <caption className={styles.hidden}>{t('wl.tokens.table.label')}</caption>
        <thead>
          <tr>
            <th scope="col" className={styles.head}>
              {t('wl.tokens.column.name')}
            </th>
            <th scope="col" className={styles.head}>
              {t('wl.tokens.column.scopes')}
            </th>
            <th scope="col" className={styles.head}>
              {t('wl.tokens.column.endpoints')}
            </th>
            <th scope="col" className={styles.head}>
              {t('wl.tokens.column.created')}
            </th>
            <th scope="col" className={styles.head}>
              {t('wl.tokens.column.lastUsed')}
            </th>
            <th scope="col" className={styles.head}>
              {t('wl.tokens.column.expires')}
            </th>
            <th scope="col" className={styles.head}>
              {t('wl.tokens.column.state')}
            </th>
            <th scope="col" className={styles.head}>
              <span className={styles.hidden}>{t('wl.tokens.column.actions')}</span>
            </th>
          </tr>
        </thead>
        <tbody>
          {tokens.map((token) => (
            <TableRow key={token.id} token={token} now={now} locale={language} onRevoke={onRevoke} />
          ))}
        </tbody>
      </table>
    </div>
  );
}
