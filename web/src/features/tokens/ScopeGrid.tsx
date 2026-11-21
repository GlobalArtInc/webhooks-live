import { useTranslation } from 'react-i18next';
import type { TokenScope } from '../../api/tokens';
import { scopeLabel } from './scopeLabels';
import styles from './ScopeGrid.module.css';

export interface ScopeGridProps {
  value: readonly TokenScope[];
  describedBy?: string;
  onToggle: (scope: TokenScope, on: boolean) => void;
}

interface ScopeCellProps {
  scope: TokenScope | null;
  value: readonly TokenScope[];
  onToggle: (scope: TokenScope, on: boolean) => void;
}

function ScopeCell({ scope, value, onToggle }: ScopeCellProps) {
  const { t } = useTranslation();
  if (scope === null) {
    return (
      <td className={styles.cell}>
        <span aria-hidden="true">-</span>
        <span className={styles.hidden}>{t('wl.tokens.grid.unavailable')}</span>
      </td>
    );
  }
  return (
    <td className={styles.cell}>
      <input
        type="checkbox"
        className={styles.checkbox}
        aria-label={scopeLabel(t, scope)}
        checked={value.includes(scope)}
        onChange={(event) => onToggle(scope, event.target.checked)}
      />
    </td>
  );
}

export function ScopeGrid({ value, describedBy, onToggle }: ScopeGridProps) {
  const { t } = useTranslation();
  return (
    <fieldset className={styles.fieldset} aria-describedby={describedBy}>
      <legend className={styles.legend}>{t('wl.tokens.grid.legend')}</legend>
      <table className={styles.table}>
        <thead>
          <tr>
            <th scope="col" className={styles.head}>
              {t('wl.tokens.grid.resource')}
            </th>
            <th scope="col" className={styles.headCell}>
              {t('wl.tokens.grid.read')}
            </th>
            <th scope="col" className={styles.headCell}>
              {t('wl.tokens.grid.write')}
            </th>
          </tr>
        </thead>
        <tbody>
          <tr>
            <th scope="row" className={styles.rowHead}>
              {t('wl.tokens.grid.everything')}
              <span className={styles.hint}>{t('wl.tokens.grid.everythingHint')}</span>
            </th>
            <ScopeCell scope="read" value={value} onToggle={onToggle} />
            <ScopeCell scope={null} value={value} onToggle={onToggle} />
          </tr>
          <tr>
            <th scope="row" className={styles.rowHead}>
              {t('wl.tokens.grid.endpoints')}
            </th>
            <ScopeCell scope={null} value={value} onToggle={onToggle} />
            <ScopeCell scope="endpoints:write" value={value} onToggle={onToggle} />
          </tr>
          <tr>
            <th scope="row" className={styles.rowHead}>
              {t('wl.tokens.grid.captures')}
            </th>
            <ScopeCell scope={null} value={value} onToggle={onToggle} />
            <ScopeCell scope="captures:write" value={value} onToggle={onToggle} />
          </tr>
          <tr>
            <th scope="row" className={styles.rowHead}>
              {t('wl.tokens.grid.rules')}
            </th>
            <ScopeCell scope={null} value={value} onToggle={onToggle} />
            <ScopeCell scope="rules:write" value={value} onToggle={onToggle} />
          </tr>
          <tr>
            <th scope="row" className={styles.rowHead}>
              {t('wl.tokens.grid.send')}
            </th>
            <ScopeCell scope={null} value={value} onToggle={onToggle} />
            <ScopeCell scope="send" value={value} onToggle={onToggle} />
          </tr>
        </tbody>
      </table>
    </fieldset>
  );
}
