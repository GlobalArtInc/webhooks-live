import { useId, useMemo, useState } from 'react';
import { useTranslation } from 'react-i18next';
import type { NameValue, SendResult } from '../../api/types';
import { downloadBlob } from '../../components/CopyAsMenu/download';
import { formatByteSize } from '../../components/format/format';
import { KeyValueTable } from '../../components/KeyValueTable/KeyValueTable';
import { StatusChip } from '../../components/StatusChip/StatusChip';
import { BodyViewer, type BodyContent, type BodyViewMode } from '../body';
import { toBodyContent } from '../detail/view';
import styles from './ResponseView.module.css';

export interface ResponseViewProps {
  result: SendResult | null;
  sending: boolean;
}

const CONTENT_TYPE_NAME = 'content-type';

function contentTypeOf(headers: readonly NameValue[]): string {
  const header = headers.find((item) => item.name.toLowerCase() === CONTENT_TYPE_NAME);
  return header === undefined ? '' : header.value;
}

function byteSizeOf(content: BodyContent): number {
  return content.type === 'text' ? new TextEncoder().encode(content.text).byteLength : content.bytes.byteLength;
}

export function ResponseView({ result, sending }: ResponseViewProps) {
  const { t, i18n } = useTranslation();
  const headingId = useId();
  const [mode, setMode] = useState<BodyViewMode>('pretty');
  const [wrap, setWrap] = useState(false);
  const [search, setSearch] = useState('');
  const content = useMemo(() => (result?.body ? toBodyContent(result.body) : null), [result]);

  function downloadRaw(): void {
    if (result === null || content === null) {
      return;
    }
    downloadBlob(new Blob([content.type === 'text' ? content.text : content.bytes]), `response-${result.id}.bin`);
  }

  return (
    <section aria-labelledby={headingId} aria-busy={sending} className={styles.section}>
      <h2 id={headingId} className={styles.heading}>
        {t('wl.composer.response.title')}
      </h2>
      {sending ? (
        <p role="status" className={styles.note}>
          {t('wl.composer.response.sending')}
        </p>
      ) : null}
      {result === null ? (
        sending ? null : (
          <p className={styles.note}>{t('wl.composer.response.none')}</p>
        )
      ) : (
        <div className={styles.result}>
          <div className={styles.summary}>
            {result.status === null ? (
              <span className={styles.note}>{t('wl.composer.response.noStatus')}</span>
            ) : (
              <StatusChip code={result.status} kind="answered" label={String(result.status)} />
            )}
            <span className={styles.note}>{t('wl.composer.response.duration', { ms: result.duration_ms })}</span>
            {content === null ? null : (
              <span className={styles.note}>
                {t('wl.composer.response.size', { size: formatByteSize(byteSizeOf(content), i18n.language) })}
              </span>
            )}
          </div>
          {result.final_url === null ? null : (
            <p className={styles.meta}>
              {t('wl.composer.response.finalUrl')}: <span className={styles.mono}>{result.final_url}</span>
            </p>
          )}
          <p className={styles.note}>{t('wl.composer.response.serverNote')}</p>
          <KeyValueTable label={t('wl.composer.response.headers')} rows={result.headers} />
          {result.body?.truncated === true ? (
            <p className={styles.note}>{t('wl.composer.response.truncated')}</p>
          ) : null}
          {content === null ? (
            <p className={styles.note}>{t('wl.composer.response.noBody')}</p>
          ) : (
            <BodyViewer
              bodyId={result.id}
              body={content}
              contentType={contentTypeOf(result.headers)}
              size={byteSizeOf(content)}
              mode={mode}
              wrap={wrap}
              search={search}
              onModeChange={setMode}
              onWrapChange={setWrap}
              onSearchChange={setSearch}
              onDownloadRaw={downloadRaw}
            />
          )}
        </div>
      )}
    </section>
  );
}
