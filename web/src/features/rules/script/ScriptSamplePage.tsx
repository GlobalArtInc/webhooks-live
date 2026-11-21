import { useRef, useState } from 'react';
import { useTranslation } from 'react-i18next';
import { ThemeControl } from '../../../components/ThemeControl/ThemeControl';
import { LanguageControl } from '../../../i18n/LanguageControl';
import type { CodeEditorHandle } from '../../editor/types';
import { fixtureScriptTransport } from './fixtureTransport';
import { SCRIPT_RECORDED_SOURCE, SCRIPT_RUNTIME_DIAGNOSTIC, SCRIPT_SAMPLE_SOURCE } from './fixtures';
import styles from './ScriptSamplePage.module.css';
import { ScriptStepEditor } from './ScriptStepEditor';
import { httpScriptTransport } from './transport';

type Variant = 'step' | 'full' | 'run';

interface VariantChoiceProps {
  id: Variant;
  label: string;
  current: Variant;
  onSelect: (variant: Variant) => void;
}

function VariantChoice({ id, label, current, onSelect }: VariantChoiceProps) {
  return (
    <label className={styles.choice}>
      <input
        type="radio"
        name="script-variant"
        value={id}
        checked={current === id}
        onChange={() => {
          onSelect(id);
        }}
      />
      {label}
    </label>
  );
}

const RUNTIME_EXTRA = [SCRIPT_RUNTIME_DIAGNOSTIC];

function useMockedServer(): boolean {
  const [mocked] = useState(() => new URLSearchParams(window.location.search).get('mock') === '1');
  return mocked;
}

export function ScriptSamplePage() {
  const { t } = useTranslation();
  const mocked = useMockedServer();
  const transport = mocked ? fixtureScriptTransport : httpScriptTransport;
  const runRef = useRef<CodeEditorHandle>(null);
  const [variant, setVariant] = useState<Variant>('step');
  const [stepSource, setStepSource] = useState(SCRIPT_SAMPLE_SOURCE);
  const [ruleSource, setRuleSource] = useState(SCRIPT_SAMPLE_SOURCE);
  const [submits, setSubmits] = useState(0);

  return (
    <main className={styles.page}>
      <header className={styles.header}>
        <h1 className={styles.title}>{t('wl.script.sample.title')}</h1>
        <LanguageControl />
        <ThemeControl />
      </header>
      <p className={styles.note}>{t('wl.script.sample.intro')}</p>
      {mocked ? <p className={styles.note}>{t('wl.script.sample.mockNote')}</p> : null}

      <fieldset className={styles.variants}>
        <legend className={styles.legend}>{t('wl.script.sample.variant.legend')}</legend>
        <VariantChoice id="step" label={t('wl.script.sample.variant.step')} current={variant} onSelect={setVariant} />
        <VariantChoice id="full" label={t('wl.script.sample.variant.full')} current={variant} onSelect={setVariant} />
        <VariantChoice id="run" label={t('wl.script.sample.variant.run')} current={variant} onSelect={setVariant} />
      </fieldset>

      {variant === 'step' ? (
        <section className={styles.section} aria-labelledby="script-step-heading">
          <h2 id="script-step-heading" className={styles.heading}>
            {t('wl.script.sample.variant.step')}
          </h2>
          <ScriptStepEditor
            label={t('wl.script.sample.stepLabel')}
            value={stepSource}
            onChange={setStepSource}
            transport={transport}
            phase="before"
            onSubmit={() => {
              setSubmits((count) => count + 1);
            }}
          />
          <p className={styles.note}>{t('wl.script.sample.submits', { count: submits })}</p>
        </section>
      ) : null}

      {variant === 'full' ? (
        <section className={styles.section} aria-labelledby="script-full-heading">
          <h2 id="script-full-heading" className={styles.heading}>
            {t('wl.script.sample.variant.full')}
          </h2>
          <ScriptStepEditor
            label={t('wl.script.sample.fullLabel')}
            value={ruleSource}
            onChange={setRuleSource}
            transport={transport}
            phase="after"
            fullHeight
            offset="24rem"
          />
        </section>
      ) : null}

      {variant === 'run' ? (
        <section className={styles.section} aria-labelledby="script-run-heading">
          <h2 id="script-run-heading" className={styles.heading}>
            {t('wl.script.sample.variant.run')}
          </h2>
          <p className={styles.banner}>{t('wl.script.sample.runBanner')}</p>
          <button
            type="button"
            className={styles.button}
            onClick={() => {
              runRef.current?.goTo(SCRIPT_RUNTIME_DIAGNOSTIC.line, SCRIPT_RUNTIME_DIAGNOSTIC.column);
            }}
          >
            {t('wl.script.sample.goRuntime')}
          </button>
          <ScriptStepEditor
            ref={runRef}
            label={t('wl.script.sample.runLabel')}
            value={SCRIPT_RECORDED_SOURCE}
            transport={transport}
            readOnly
            extraDiagnostics={RUNTIME_EXTRA}
          />
        </section>
      ) : null}
    </main>
  );
}
