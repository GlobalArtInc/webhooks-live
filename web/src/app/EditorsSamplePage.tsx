import { useMemo, useRef, useState } from 'react';
import { useTranslation } from 'react-i18next';
import { CapturePicker, type CapturePick } from '../components/CapturePicker/CapturePicker';
import { ChipInput } from '../components/ChipInput/ChipInput';
import { CodeEditor } from '../components/CodeEditor/CodeEditor';
import { KeyValueEditor } from '../components/KeyValueEditor/KeyValueEditor';
import type { KeyValueRow, KeyValueRowError } from '../components/KeyValueEditor/rows';
import { ThemeControl } from '../components/ThemeControl/ThemeControl';
import { VariablePicker } from '../components/VariablePicker/VariablePicker';
import { variableCompletionSource } from '../components/VariablePicker/variables';
import type { CodeEditorHandle } from '../features/editor/types';
import { LanguageControl } from '../i18n/LanguageControl';
import {
  SAMPLE_BODY,
  SAMPLE_CAPTURES,
  SAMPLE_HEADER_ROWS,
  SAMPLE_NOW_MS,
  SAMPLE_READ_ONLY_BODY,
  SAMPLE_SCRIPT,
  SAMPLE_SENDERS,
  SAMPLE_SENDER_SUGGESTIONS,
  SCRIPT_COMPLETIONS,
  VARIABLE_CATALOGUE,
  scriptDiagnostics,
  templateDiagnostics,
} from './editorsSampleData';
import styles from './EditorsSamplePage.module.css';

const BODY_LIMIT_BYTES = 64 * 1024;
const CIDR = /^(?:\d{1,3}(?:\.\d{1,3}){3}|[0-9a-fA-F:]+)\/\d{1,3}$/;

function validateCidr(value: string): string | null {
  if (!CIDR.test(value)) {
    return `${value} is not an address range`;
  }
  const octets = value.split('/')[0]?.split('.') ?? [];
  if (octets.length === 4 && octets.some((octet) => Number(octet) > 255)) {
    return `${value} has an octet above 255`;
  }
  return null;
}

function headerErrors(rows: readonly KeyValueRow[]): KeyValueRowError[] {
  return rows
    .filter((row) => /\s/.test(row.name))
    .map((row) => ({ rowId: row.id, field: 'name', message: 'Header names cannot contain spaces.' }));
}

export function EditorsSamplePage() {
  const { t } = useTranslation();
  const bodyRef = useRef<CodeEditorHandle>(null);
  const [body, setBody] = useState(SAMPLE_BODY);
  const [script, setScript] = useState(SAMPLE_SCRIPT);
  const [rows, setRows] = useState<KeyValueRow[]>([...SAMPLE_HEADER_ROWS]);
  const [senders, setSenders] = useState<string[]>([...SAMPLE_SENDERS]);
  const [pick, setPick] = useState<CapturePick>({ mode: 'latest' });
  const [inserted, setInserted] = useState('');
  const [submits, setSubmits] = useState(0);
  const [pickerInserted, setPickerInserted] = useState('');

  const bodyDiagnostics = useMemo(() => templateDiagnostics(body), [body]);
  const scriptProblems = useMemo(() => scriptDiagnostics(script), [script]);
  const bodyCompletions = useMemo(() => variableCompletionSource(VARIABLE_CATALOGUE), []);
  const scriptCompletions = useMemo(() => () => SCRIPT_COMPLETIONS, []);
  const errors = useMemo(() => headerErrors(rows), [rows]);
  const invalidSenders = useMemo(() => senders.filter((sender) => validateCidr(sender) !== null), [senders]);

  return (
    <main className={styles.page}>
      <header className={styles.header}>
        <h1 className={styles.title}>{t('wl.editorsample.title')}</h1>
        <LanguageControl />
        <ThemeControl />
      </header>

      <section className={styles.section} aria-labelledby="sample-body">
        <h2 id="sample-body" className={styles.heading}>
          {t('wl.editorsample.body.title')}
        </h2>
        <div className={styles.toolbar}>
          <VariablePicker
            catalogue={VARIABLE_CATALOGUE}
            sampleCapture="POST /stripe"
            onInsert={(entry) => {
              bodyRef.current?.insertAtCursor(entry.insertText);
              setInserted(entry.insertText);
            }}
            onCloseFocus={() => {
              bodyRef.current?.focus();
            }}
          />
          <span className={styles.note}>{t('wl.editorsample.send', { count: submits })}</span>
        </div>
        <CodeEditor
          ref={bodyRef}
          label={t('wl.editorsample.body.title')}
          language="json"
          delimiters={{ open: '{{', close: '}}' }}
          value={body}
          onChange={setBody}
          diagnostics={bodyDiagnostics}
          completions={bodyCompletions}
          maxBytes={BODY_LIMIT_BYTES}
          onSubmit={() => {
            setSubmits((count) => count + 1);
          }}
        />
        {inserted === '' ? null : (
          <p className={styles.note}>{t('wl.editorsample.insertedInto', { text: inserted })}</p>
        )}
      </section>

      <section className={styles.section} aria-labelledby="sample-headers">
        <h2 id="sample-headers" className={styles.heading}>
          {t('wl.editorsample.headers.title')}
        </h2>
        <KeyValueEditor
          label={t('wl.editorsample.header.title')}
          rows={rows}
          onChange={setRows}
          errors={errors}
          maxRows={5}
          maxNameLength={40}
          nameSuggestions={['content-type', 'x-order-id', 'cache-control']}
          renderValue={(field) => (
            <CodeEditor
              label={field.label}
              labelHidden
              language="template"
              singleLine
              readOnly={field.readOnly}
              value={field.row.value}
              onChange={field.onChange}
              completions={bodyCompletions}
            />
          )}
        />
      </section>

      <section className={styles.section} aria-labelledby="sample-script">
        <h2 id="sample-script" className={styles.heading}>
          {t('wl.editorsample.script.title')}
        </h2>
        <CodeEditor
          label={t('wl.editorsample.script.title')}
          language="script"
          value={script}
          onChange={setScript}
          diagnostics={scriptProblems}
          completions={scriptCompletions}
          maxBytes={16 * 1024}
          rows={8}
        />
      </section>

      <section className={styles.section} aria-labelledby="sample-readonly">
        <h2 id="sample-readonly" className={styles.heading}>
          {t('wl.editorsample.readonly.title')}
        </h2>
        <CodeEditor
          label={t('wl.editorsample.readonly.title')}
          language="json"
          value={SAMPLE_READ_ONLY_BODY}
          readOnly
          rows={4}
        />
      </section>

      <section className={styles.section} aria-labelledby="sample-chips">
        <h2 id="sample-chips" className={styles.heading}>
          {t('wl.editorsample.chips.title')}
        </h2>
        <ChipInput
          label={t('wl.editorsample.chips.title')}
          values={senders}
          onChange={setSenders}
          validate={validateCidr}
          invalidValues={invalidSenders}
          suggestions={SAMPLE_SENDER_SUGGESTIONS}
          maxItems={8}
          maxLength={43}
        />
      </section>

      <section className={styles.section} aria-labelledby="sample-pickers">
        <h2 id="sample-pickers" className={styles.heading}>
          {t('wl.editorsample.picker.title')}
        </h2>
        <div className={styles.toolbar}>
          <VariablePicker
            catalogue={VARIABLE_CATALOGUE}
            sampleCapture="POST /stripe"
            compact
            onInsert={(entry) => {
              setPickerInserted(entry.insertText);
            }}
          />
          <VariablePicker
            catalogue={[]}
            loading
            onInsert={() => undefined}
            triggerLabel={t('wl.editorsample.loading')}
          />
          {pickerInserted === '' ? null : <code className={styles.note}>{pickerInserted}</code>}
        </div>
        <CapturePicker
          captures={SAMPLE_CAPTURES}
          value={pick}
          onChange={setPick}
          onSendSample={() => undefined}
          now={SAMPLE_NOW_MS}
        />
        <CapturePicker
          captures={[]}
          value={{ mode: 'latest' }}
          onChange={() => undefined}
          onSendSample={() => undefined}
          now={SAMPLE_NOW_MS}
        />
        <CapturePicker
          captures={undefined}
          value={{ mode: 'latest' }}
          onChange={() => undefined}
          onSendSample={() => undefined}
          now={SAMPLE_NOW_MS}
        />
      </section>
    </main>
  );
}
