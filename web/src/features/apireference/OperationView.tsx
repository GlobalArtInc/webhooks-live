import { useNavigate } from '@tanstack/react-router';
import { memo, useMemo } from 'react';
import { useTranslation } from 'react-i18next';
import { MethodBadge } from '../../components/MethodBadge/MethodBadge';
import { stashComposerSeed } from '../composer/seedHandoff';
import { operationAnchorId, operationHeadingId } from './anchors';
import { composerSeedFor } from './composerSeed';
import { CopyAction } from './CopyAction';
import { ParameterTable } from './ParameterTable';
import { operationLabel, type ReferenceMedia, type ReferenceOperation, type ReferenceResponse } from './reference';
import { buildSample, type SampleContext } from './samples';
import { SamplePanel } from './SamplePanel';
import { SchemaBlock } from './SchemaView';
import { SecurityChips } from './SecurityChips';
import { TextBlocks } from './TextBlocks';
import styles from './OperationView.module.css';

export interface OperationViewProps {
  operation: ReferenceOperation;
  context: SampleContext;
}

interface MediaListProps {
  content: readonly ReferenceMedia[];
  name: string;
  defaultOpen: boolean;
}

interface ResponseItemProps {
  response: ReferenceResponse;
  operationName: string;
}

function MediaList({ content, name, defaultOpen }: MediaListProps) {
  return (
    <ul className={styles.media}>
      {content.map((media) => (
        <li key={media.mediaType} className={styles.mediaItem}>
          <code className={styles.mediaType}>{media.mediaType}</code>
          {media.schema === null ? null : (
            <SchemaBlock node={media.schema} name={`${name} ${media.mediaType}`} defaultOpen={defaultOpen} />
          )}
        </li>
      ))}
    </ul>
  );
}

function ResponseItem({ response, operationName }: ResponseItemProps) {
  const { t } = useTranslation();
  const name = `${operationName} ${response.status}`;
  return (
    <li className={styles.response}>
      <div className={styles.responseHead}>
        <span className={styles.status}>{response.status}</span>
        <div className={styles.responseText}>
          <TextBlocks text={response.description} />
        </div>
      </div>
      {response.content.length === 0 ? null : <MediaList content={response.content} name={name} defaultOpen={false} />}
      {response.headers.length === 0 ? null : (
        <p className={styles.headers}>
          {t('wl.apireference.operation.responseHeaders')}
          {': '}
          {response.headers.map((header) => header.name).join(', ')}
        </p>
      )}
    </li>
  );
}

function OperationViewBase({ operation, context }: OperationViewProps) {
  const { t } = useTranslation();
  const navigate = useNavigate();
  const label = operationLabel(operation);
  const sample = useMemo(() => buildSample(operation, context), [operation, context]);
  const linkUrl = `${context.origin}/docs/api/${encodeURIComponent(operation.id)}`;
  const headingId = operationHeadingId(operation.id);

  function openInComposer(): void {
    stashComposerSeed(composerSeedFor(sample));
    void navigate({ to: '/send' });
  }

  return (
    <article id={operationAnchorId(operation.id)} className={styles.operation} aria-labelledby={headingId}>
      <header className={styles.header}>
        <h3 id={headingId} className={styles.heading} tabIndex={-1}>
          <MethodBadge method={operation.method} /> <span className={styles.path}>{operation.path}</span>
        </h3>
        {operation.deprecated ? (
          <span className={styles.deprecated}>{t('wl.apireference.operation.deprecated')}</span>
        ) : null}
        <div className={styles.actions}>
          <CopyAction value={linkUrl} label={t('wl.apireference.operation.copyLinkLabel', { operation: label })} />
          <button
            type="button"
            className={styles.compose}
            aria-label={t('wl.apireference.operation.openInComposerLabel', { operation: label })}
            onClick={openInComposer}
          >
            {t('wl.apireference.operation.openInComposer')}
          </button>
        </div>
      </header>
      <div className={styles.body}>
        {operation.summary === null ? null : <p className={styles.summary}>{operation.summary}</p>}
        {operation.description === null || operation.description === operation.summary ? null : (
          <div className={styles.description}>
            <TextBlocks text={operation.description} />
          </div>
        )}
        <div role="group" className={styles.section} aria-labelledby={`${headingId}-auth`}>
          <h4 id={`${headingId}-auth`} className={styles.label}>
            {t('wl.apireference.operation.auth')}
          </h4>
          <SecurityChips chips={operation.security} />
        </div>
        {operation.parameters.length === 0 ? null : (
          <div role="group" className={styles.section} aria-labelledby={`${headingId}-parameters`}>
            <h4 id={`${headingId}-parameters`} className={styles.label}>
              {t('wl.apireference.operation.parameters')}
            </h4>
            <ParameterTable
              parameters={operation.parameters}
              caption={`${label}: ${t('wl.apireference.operation.parameters')}`}
            />
          </div>
        )}
        {operation.requestBody === null ? null : (
          <div role="group" className={styles.section} aria-labelledby={`${headingId}-body`}>
            <h4 id={`${headingId}-body`} className={styles.label}>
              {t('wl.apireference.operation.requestBody')}
              {operation.requestBody.required ? ` (${t('wl.apireference.operation.bodyRequired')})` : null}
            </h4>
            {operation.requestBody.description === null ? null : (
              <div className={styles.description}>
                <TextBlocks text={operation.requestBody.description} />
              </div>
            )}
            <MediaList
              content={operation.requestBody.content}
              name={`${label} ${t('wl.apireference.operation.requestBody')}`}
              defaultOpen
            />
          </div>
        )}
        {operation.responses.length === 0 ? null : (
          <div role="group" className={styles.section} aria-labelledby={`${headingId}-responses`}>
            <h4 id={`${headingId}-responses`} className={styles.label}>
              {t('wl.apireference.operation.responses')}
            </h4>
            <ul className={styles.responses}>
              {operation.responses.map((response) => (
                <ResponseItem key={response.status} response={response} operationName={label} />
              ))}
            </ul>
          </div>
        )}
        <div role="group" className={styles.section} aria-labelledby={`${headingId}-example`}>
          <h4 id={`${headingId}-example`} className={styles.label}>
            {t('wl.apireference.operation.example')}
          </h4>
          <SamplePanel sample={sample} operationLabel={label} />
        </div>
      </div>
    </article>
  );
}

export const OperationView = memo(OperationViewBase);
