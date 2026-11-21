import { useState } from 'react';
import { useTranslation } from 'react-i18next';
import { useEndpoint, useInstance } from '../../api/queries';
import type { SectionProps } from '../workspace/sections';
import { ForwardRuleEditor } from './ForwardRuleEditor';
import { ForwardRuleItem } from './ForwardRuleItem';
import styles from './ForwardingSection.module.css';

export function ForwardingSection({ endpointId }: SectionProps) {
  const { t } = useTranslation();
  const endpoint = useEndpoint(endpointId).data;
  const instance = useInstance().data;
  const [adding, setAdding] = useState(false);

  if (endpoint === undefined) {
    return (
      <p role="status" className={styles.loading}>
        {t('wl.workspace.loading')}
      </p>
    );
  }

  const rules = endpoint.forward_rules;
  const max = instance?.limits.max_forward_rules ?? rules.length;
  const full = rules.length >= max;
  const readOnly = endpoint.my_role === 'viewer';

  return (
    <div className={styles.page}>
      <h2 className={styles.heading}>{t('wl.workspace.section.forwarding')}</h2>
      <p className={styles.hint}>{t('wl.forwarding.description')}</p>
      <p className={styles.count}>{t('wl.forwarding.count', { used: rules.length, max })}</p>
      {rules.length === 0 && !adding ? <p className={styles.empty}>{t('wl.forwarding.empty')}</p> : null}
      {rules.length === 0 ? null : (
        <ul className={styles.list}>
          {rules.map((rule) => (
            <li key={rule.id}>
              <ForwardRuleItem endpointId={endpointId} rule={rule} readOnly={readOnly} />
            </li>
          ))}
        </ul>
      )}
      {adding ? (
        <ForwardRuleEditor
          endpointId={endpointId}
          rule={null}
          onDone={() => {
            setAdding(false);
          }}
          onCancel={() => {
            setAdding(false);
          }}
        />
      ) : null}
      <div className={styles.footer}>
        <button
          type="button"
          className={styles.add}
          disabled={readOnly || full || adding}
          onClick={() => {
            setAdding(true);
          }}
        >
          {t('wl.forwarding.add')}
        </button>
        {full ? <p className={styles.hint}>{t('wl.forwarding.limit')}</p> : null}
      </div>
    </div>
  );
}
