import { useId, type KeyboardEvent } from 'react';
import { useTranslation } from 'react-i18next';
import { CodeEditor } from '../../components/CodeEditor/CodeEditor';
import { KeyValueEditor } from '../../components/KeyValueEditor/KeyValueEditor';
import type { KeyValueRowError } from '../../components/KeyValueEditor/rows';
import {
  BODY_MAX_BYTES,
  TIMEOUT_MAX_MS,
  contentTypeOf,
  contentTypeOptions,
  languageOf,
  methodOptions,
  withContentType,
  type ComposerDraft,
} from './draft';
import { useComposerMessages } from './messages';
import type { UrlIssue } from './sendErrors';
import styles from './RequestEditor.module.css';

export interface RequestEditorProps {
  draft: ComposerDraft;
  onChange: (draft: ComposerDraft) => void;
  urlIssue: UrlIssue | null;
  redactedIds: readonly string[];
  sending: boolean;
  onSend: () => void;
  onCancel: () => void;
}

export function RequestEditor({
  draft,
  onChange,
  urlIssue,
  redactedIds,
  sending,
  onSend,
  onCancel,
}: RequestEditorProps) {
  const { t } = useTranslation();
  const { urlText } = useComposerMessages();
  const baseId = useId();
  const methodId = `${baseId}-method`;
  const urlId = `${baseId}-url`;
  const errorId = `${baseId}-url-error`;
  const typeId = `${baseId}-type`;
  const timeoutId = `${baseId}-timeout`;
  const timeoutHintId = `${baseId}-timeout-hint`;
  const redirectsId = `${baseId}-redirects`;
  const contentType = contentTypeOf(draft.headers);
  const headerErrors: KeyValueRowError[] = redactedIds.map((rowId) => ({
    rowId,
    field: 'value',
    message: t('wl.composer.redacted'),
  }));

  function submitFromUrl(event: KeyboardEvent<HTMLInputElement>): void {
    if (event.key === 'Enter') {
      event.preventDefault();
      onSend();
    }
  }

  return (
    <section aria-labelledby={`${baseId}-heading`} className={styles.section}>
      <h2 id={`${baseId}-heading`} className={styles.heading}>
        {t('wl.composer.request')}
      </h2>
      <p className={styles.intro}>{t('wl.composer.intro')}</p>
      <div className={styles.target}>
        <div className={styles.field}>
          <label htmlFor={methodId} className={styles.label}>
            {t('wl.composer.method')}
          </label>
          <select
            id={methodId}
            className={styles.select}
            value={draft.method}
            onChange={(event) => {
              onChange({ ...draft, method: event.target.value });
            }}
          >
            {methodOptions(draft.method).map((method) => (
              <option key={method} value={method}>
                {method}
              </option>
            ))}
          </select>
        </div>
        <div className={styles.urlField}>
          <label htmlFor={urlId} className={styles.label}>
            {t('wl.composer.url')}
          </label>
          <input
            id={urlId}
            type="text"
            inputMode="url"
            spellCheck={false}
            autoComplete="off"
            placeholder={t('wl.composer.url.placeholder')}
            className={styles.input}
            aria-invalid={urlIssue === null ? undefined : true}
            aria-describedby={urlIssue === null ? undefined : errorId}
            value={draft.url}
            onKeyDown={submitFromUrl}
            onChange={(event) => {
              onChange({ ...draft, url: event.target.value });
            }}
          />
          {urlIssue === null ? null : (
            <p id={errorId} role="alert" className={styles.error}>
              {urlText(urlIssue)}
            </p>
          )}
        </div>
        {sending ? (
          <button type="button" className={styles.secondary} onClick={onCancel}>
            {t('wl.composer.cancel')}
          </button>
        ) : (
          <button type="button" className={styles.primary} onClick={onSend}>
            {t('wl.composer.send')}
          </button>
        )}
      </div>
      <KeyValueEditor
        label={t('wl.composer.headers')}
        rows={draft.headers}
        errors={headerErrors}
        onChange={(headers) => {
          onChange({ ...draft, headers });
        }}
      />
      <div className={styles.field}>
        <label htmlFor={typeId} className={styles.label}>
          {t('wl.composer.contentType')}
        </label>
        <select
          id={typeId}
          className={styles.select}
          value={contentType}
          onChange={(event) => {
            onChange({ ...draft, headers: withContentType(draft.headers, event.target.value) });
          }}
        >
          {contentTypeOptions(contentType).map((value) => (
            <option key={value} value={value}>
              {value === '' ? t('wl.composer.contentType.none') : value}
            </option>
          ))}
        </select>
      </div>
      <CodeEditor
        value={draft.body.data}
        language={languageOf(contentType)}
        label={t('wl.composer.body')}
        maxBytes={BODY_MAX_BYTES}
        rows={10}
        onChange={(data) => {
          onChange({ ...draft, body: { encoding: 'utf8', data, truncated: false } });
        }}
        onSubmit={onSend}
      />
      {draft.body.encoding === 'base64' ? <p className={styles.note}>{t('wl.composer.body.binary')}</p> : null}
      {draft.body.truncated ? <p className={styles.note}>{t('wl.composer.body.truncated')}</p> : null}
      <fieldset className={styles.options}>
        <legend className={styles.heading}>{t('wl.composer.options')}</legend>
        <div className={styles.field}>
          <label htmlFor={timeoutId} className={styles.label}>
            {t('wl.composer.timeout')}
          </label>
          <input
            id={timeoutId}
            type="number"
            inputMode="numeric"
            min={1}
            max={TIMEOUT_MAX_MS}
            aria-describedby={timeoutHintId}
            className={styles.input}
            value={draft.timeout}
            onChange={(event) => {
              onChange({ ...draft, timeout: event.target.value });
            }}
          />
          <p id={timeoutHintId} className={styles.hint}>
            {t('wl.composer.timeout.hint')}
          </p>
        </div>
        <div className={styles.check}>
          <input
            id={redirectsId}
            type="checkbox"
            checked={draft.followRedirects}
            onChange={(event) => {
              onChange({ ...draft, followRedirects: event.target.checked });
            }}
          />
          <label htmlFor={redirectsId}>{t('wl.composer.followRedirects')}</label>
        </div>
      </fieldset>
    </section>
  );
}
