import { useState } from 'react';
import { useTranslation } from 'react-i18next';
import { ApiError } from '../../api/client';
import { CapturePicker } from '../../components/CapturePicker/CapturePicker';
import { formatByteSize } from '../../components/format/format';
import { useNow } from '../endpoints/useNow';
import styles from './TemplatePreview.module.css';
import type { RenderedResponse } from './types';
import type { TemplatingModel } from './useTemplating';

export interface TemplatePreviewProps {
  model: TemplatingModel;
}

type BodyView = 'pretty' | 'raw';

interface BodyText {
  text: string;
  prettyAvailable: boolean;
  binary: boolean;
}

function failureText(failure: unknown): string {
  if (failure instanceof ApiError || failure instanceof Error) {
    return failure.message;
  }
  return '';
}

function isJsonType(contentType: string): boolean {
  return contentType.split(';')[0]?.toLowerCase().includes('json') === true;
}

function prettyJson(text: string): string | null {
  try {
    return JSON.stringify(JSON.parse(text), null, 2);
  } catch (failure: unknown) {
    if (failure instanceof SyntaxError) {
      return null;
    }
    throw failure;
  }
}

export function bodyTextOf(rendered: RenderedResponse, view: BodyView): BodyText {
  if (rendered.body.encoding === 'base64') {
    return { text: '', prettyAvailable: false, binary: true };
  }
  const pretty = isJsonType(rendered.content_type) ? prettyJson(rendered.body.data) : null;
  const useful = pretty !== null && pretty !== rendered.body.data;
  return {
    text: view === 'pretty' && pretty !== null ? pretty : rendered.body.data,
    prettyAvailable: useful,
    binary: false,
  };
}

interface RenderedViewProps {
  rendered: RenderedResponse;
}

function RenderedView({ rendered }: RenderedViewProps) {
  const { t, i18n } = useTranslation();
  const [view, setView] = useState<BodyView>('pretty');
  const body = bodyTextOf(rendered, view);
  const size = formatByteSize(rendered.body_size, i18n.language);
  return (
    <div className={styles.rendered}>
      <p className={styles.summary}>
        {t('wl.templating.preview.summary', {
          status: rendered.status,
          contentType: rendered.content_type,
          size,
          duration: rendered.duration_ms,
        })}
      </p>
      <h4 className={styles.subheading}>{t('wl.templating.preview.headers')}</h4>
      {rendered.headers.length === 0 ? (
        <p className={styles.note}>{t('wl.templating.preview.noHeaders')}</p>
      ) : (
        <dl className={styles.headers}>
          {rendered.headers.map((header, index) => (
            <div key={`${header.name}:${index}`} className={styles.header}>
              <dt className={styles.headerName}>{header.name}</dt>
              <dd className={styles.headerValue}>{header.value}</dd>
            </div>
          ))}
        </dl>
      )}
      <div className={styles.bodyHeading}>
        <h4 className={styles.subheading}>{t('wl.templating.preview.body')}</h4>
        {body.prettyAvailable ? (
          <div role="group" aria-label={t('wl.templating.preview.format')} className={styles.formats}>
            <button
              type="button"
              className={styles.format}
              aria-pressed={view === 'pretty'}
              onClick={() => {
                setView('pretty');
              }}
            >
              {t('wl.templating.preview.pretty')}
            </button>
            <button
              type="button"
              className={styles.format}
              aria-pressed={view === 'raw'}
              onClick={() => {
                setView('raw');
              }}
            >
              {t('wl.templating.preview.raw')}
            </button>
          </div>
        ) : null}
      </div>
      {body.binary ? (
        <p className={styles.note}>{t('wl.templating.preview.binary', { size })}</p>
      ) : rendered.body.data === '' ? (
        <p className={styles.note}>{t('wl.templating.preview.emptyBody')}</p>
      ) : (
        <pre className={styles.body} tabIndex={0}>
          {body.text}
        </pre>
      )}
      {rendered.body.truncated ? <p className={styles.note}>{t('wl.templating.preview.truncated')}</p> : null}
    </div>
  );
}

export function TemplatePreview({ model }: TemplatePreviewProps) {
  const { t } = useTranslation();
  const now = useNow();
  const { state, stale, refresh } = model.preview;
  const rendered = state.result?.rendered ?? state.lastRendered;
  const rendering = state.phase === 'rendering';
  const failed = state.phase === 'failed';
  const hasErrors = model.errorCount > 0;

  if (!model.enabled) {
    return <p className={styles.note}>{t('wl.templating.preview.off')}</p>;
  }

  let statusText = '';
  if (rendering) {
    statusText = t('wl.templating.preview.rendering');
  } else if (stale && state.phase !== 'failed') {
    statusText = t('wl.templating.preview.stale');
  }

  return (
    <div className={styles.preview}>
      <div className={styles.controls}>
        <CapturePicker
          captures={model.captures}
          value={model.pick}
          onChange={model.onPick}
          onSendSample={model.sendSample}
          sendingSample={model.sendingSample}
          now={now}
        />
        <button type="button" className={styles.refresh} onClick={refresh}>
          {t('wl.templating.preview.refresh')}
        </button>
      </div>
      {model.sampleError === null || model.sampleError === undefined ? null : (
        <p role="alert" className={styles.error}>
          {t('wl.templating.preview.sampleFailed', { message: failureText(model.sampleError) })}
        </p>
      )}
      <p role="status" className={styles.status}>
        {statusText}
      </p>
      {failed ? (
        <p role="alert" className={styles.error}>
          {t('wl.templating.preview.failed', { message: failureText(state.error) })}
        </p>
      ) : null}
      {hasErrors && rendered !== null ? (
        <p className={styles.warning}>{t('wl.templating.preview.lastValid', { count: model.errorCount })}</p>
      ) : null}
      {hasErrors && rendered === null ? <p className={styles.note}>{t('wl.templating.preview.noRender')}</p> : null}
      {!hasErrors && rendered === null && !failed && !rendering && !model.hasCapture ? (
        <p className={styles.note}>{t('wl.templating.preview.noCapture')}</p>
      ) : null}
      {rendered === null ? null : <RenderedView rendered={rendered} />}
    </div>
  );
}
