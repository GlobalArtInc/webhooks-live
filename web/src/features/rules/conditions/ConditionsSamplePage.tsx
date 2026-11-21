import { useState } from 'react';
import { useTranslation } from 'react-i18next';
import { ThemeControl } from '../../../components/ThemeControl/ThemeControl';
import { LanguageControl } from '../../../i18n/LanguageControl';
import styles from './ConditionsSamplePage.module.css';
import { CONDITION_CATALOGUE, SAMPLE_VARIABLES, UNSUPPORTED_CONDITION, sampleSource } from './fixtures';
import { RuleConditionsEditor } from './RuleConditionsEditor';
import { createSampleServer } from './sampleServer';
import type { ConditionsInput, RuleConditionsSource } from './types';

export function ConditionsSamplePage() {
  const { t } = useTranslation();
  const [server] = useState(() => createSampleServer(sampleSource()));
  const [source, setSource] = useState<RuleConditionsSource>(() => server.read());
  const [lastBody, setLastBody] = useState('');
  const [readOnly, setReadOnly] = useState(false);
  const [unsupported, setUnsupported] = useState(false);
  const shown: RuleConditionsSource = unsupported ? { ...source, condition: UNSUPPORTED_CONDITION } : source;

  async function save(input: ConditionsInput, version: number): Promise<RuleConditionsSource> {
    setLastBody(JSON.stringify(input, null, 2));
    const saved = await server.save(input, version);
    setSource(saved);
    return saved;
  }

  return (
    <main className={styles.page}>
      <header className={styles.header}>
        <h1 className={styles.title}>{t('wl.conditions.sample.title')}</h1>
        <LanguageControl />
        <ThemeControl />
      </header>
      <div className={styles.toolbar}>
        <button
          type="button"
          className={styles.button}
          onClick={() => {
            setSource(server.touch());
          }}
        >
          {t('wl.conditions.sample.touch')}
        </button>
        <label className={styles.toggle}>
          <input
            type="checkbox"
            checked={readOnly}
            onChange={(event) => {
              setReadOnly(event.target.checked);
            }}
          />
          <span>{t('wl.conditions.sample.readOnly')}</span>
        </label>
        <label className={styles.toggle}>
          <input
            type="checkbox"
            checked={unsupported}
            onChange={(event) => {
              setUnsupported(event.target.checked);
            }}
          />
          <span>{t('wl.conditions.sample.unsupported')}</span>
        </label>
        <span className={styles.note}>{t('wl.conditions.sample.version', { version: source.version })}</span>
      </div>
      <section className={styles.panel}>
        <RuleConditionsEditor
          key={unsupported ? 'unsupported' : 'supported'}
          source={shown}
          catalogue={CONDITION_CATALOGUE}
          variables={SAMPLE_VARIABLES}
          save={save}
          onConflict={() => {
            setSource(server.read());
          }}
          readOnly={readOnly}
        />
      </section>
      {lastBody === '' ? null : (
        <section className={styles.panel} aria-labelledby="sample-last-body">
          <h2 id="sample-last-body" className={styles.heading}>
            {t('wl.conditions.sample.lastBody')}
          </h2>
          <pre className={styles.body}>{lastBody}</pre>
        </section>
      )}
    </main>
  );
}
