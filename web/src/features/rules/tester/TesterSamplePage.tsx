import { useMemo, useRef, useState } from 'react';
import { useTranslation } from 'react-i18next';
import { ThemeControl } from '../../../components/ThemeControl/ThemeControl';
import { LanguageControl } from '../../../i18n/LanguageControl';
import { fixtureTesterTransport } from './fixtureTransport';
import { TESTER_DRAFT_INPUT, TESTER_ENDPOINT_ID, TESTER_ENDPOINT_URL, TESTER_RULE_IDS } from './fixtures';
import { RuleTester } from './RuleTester';
import styles from './TesterSamplePage.module.css';
import { httpTesterTransport } from './transport';
import { stepsOf } from './request';
import type { TesterHandle } from './props';
import type { TesterDraft, TraceSelection } from './types';

const DEFAULT_BODY = '{"ok":true}';

function useMockedServer(): boolean {
  const [mocked] = useState(() => new URLSearchParams(window.location.search).get('mock') === '1');
  return mocked;
}

function draftOf(body: string): TesterDraft {
  const steps = stepsOf(TESTER_DRAFT_INPUT).map((step) => (step.kind === 'respond' ? { ...step, body } : step));
  return { ruleId: TESTER_RULE_IDS.paid, input: { ...TESTER_DRAFT_INPUT, steps } };
}

export function TesterSamplePage() {
  const { t } = useTranslation();
  const mocked = useMockedServer();
  const transport = mocked ? fixtureTesterTransport : httpTesterTransport;
  const handle = useRef<TesterHandle>(null);
  const [body, setBody] = useState(DEFAULT_BODY);
  const [blocked, setBlocked] = useState(false);
  const [viewer, setViewer] = useState(false);
  const [selection, setSelection] = useState<TraceSelection | null>(null);
  const draft = useMemo(() => draftOf(body), [body]);
  const steps = stepsOf(draft.input);
  const selectedStep = selection?.stepIndex ?? null;
  const conditionsSelected = selection !== null && selection.stepIndex === null;

  return (
    <main className={styles.page}>
      <header className={styles.header}>
        <h1 className={styles.title}>{t('wl.tester.sample.title')}</h1>
        <LanguageControl />
        <ThemeControl />
      </header>

      <section className={styles.panel} aria-labelledby="tester-sample-draft">
        <h2 id="tester-sample-draft" className={styles.heading}>
          {t('wl.tester.sample.draft')}
        </h2>
        <label className={styles.field}>
          <span>{t('wl.tester.sample.body')}</span>
          <input
            className={styles.input}
            type="text"
            value={body}
            onChange={(event) => {
              setBody(event.target.value);
            }}
          />
        </label>
        <label className={styles.toggle}>
          <input
            type="checkbox"
            checked={blocked}
            onChange={(event) => {
              setBlocked(event.target.checked);
            }}
          />
          <span>{t('wl.tester.sample.blocked')}</span>
        </label>
        <label className={styles.toggle}>
          <input
            type="checkbox"
            checked={viewer}
            onChange={(event) => {
              setViewer(event.target.checked);
            }}
          />
          <span>{t('wl.tester.sample.viewer')}</span>
        </label>
        <button
          type="button"
          className={styles.button}
          onClick={() => {
            handle.current?.selectStep(1);
          }}
        >
          {t('wl.tester.sample.selectStep')}
        </button>
        <ol className={styles.cards} aria-label={t('wl.tester.sample.cards')}>
          <li className={styles.card} data-highlighted={conditionsSelected}>
            {t('wl.tester.sample.conditionsCard')}
          </li>
          {steps.map((step, index) => (
            <li key={`${String(step.kind)}-${index}`} className={styles.card} data-highlighted={selectedStep === index}>
              {t('wl.tester.sample.stepCard', { position: index + 1, kind: String(step.kind) })}
            </li>
          ))}
        </ol>
        <p className={styles.note} data-testid="tester-selection">
          {selection === null
            ? t('wl.tester.sample.noSelection')
            : selection.stepIndex === null
              ? t('wl.tester.sample.selectedConditions')
              : t('wl.tester.sample.selectedStep', { position: selection.stepIndex + 1 })}
        </p>
      </section>

      <RuleTester
        ref={handle}
        endpointId={TESTER_ENDPOINT_ID}
        endpointUrl={TESTER_ENDPOINT_URL}
        draft={draft}
        transport={transport}
        blocked={blocked}
        canEdit={!viewer}
        dirty={body !== DEFAULT_BODY}
        onSelectionChange={setSelection}
      />
    </main>
  );
}
