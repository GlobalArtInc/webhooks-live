import { useCallback, useState } from 'react';
import { useTranslation } from 'react-i18next';
import { ApiError, fetchCaptureBody } from '../../api/client';
import { useCapture } from '../../api/queries';
import type { Capture } from '../../api/types';
import { downloadBlob } from '../../components/CopyAsMenu/download';
import type { Bytes } from '../body';
import { CaptureDetail, type CaptureDetailProps } from './CaptureDetail';
import type { CaptureDetailState } from './types';

const NOT_FOUND = 404;
const RAW_FILE_EXTENSION = 'bin';

export interface CaptureDetailPanelProps extends Omit<
  CaptureDetailProps,
  'state' | 'capture' | 'loadBody' | 'onDownloadRaw'
> {
  endpointId: string;
  captureId: string;
}

async function loadBodyBytes(endpointId: string, captureId: string, limit: number | null): Promise<Bytes> {
  const blob = await fetchCaptureBody(endpointId, captureId);
  const bytes: Bytes = new Uint8Array(await blob.arrayBuffer());
  return limit === null ? bytes : bytes.subarray(0, limit);
}

function stateOf(capture: Capture | undefined, error: Error | null): CaptureDetailState {
  if (error !== null) {
    return error instanceof ApiError && error.status === NOT_FOUND ? 'deleted' : 'error';
  }
  if (capture === undefined) {
    return 'loading';
  }
  return capture.body.truncated ? 'large-pending' : 'ready';
}

export function CaptureDetailPanel({ endpointId, captureId, ...rest }: CaptureDetailPanelProps) {
  const { t } = useTranslation();
  const query = useCapture(endpointId, captureId);
  const [downloadFailed, setDownloadFailed] = useState(false);

  const loadBody = useCallback(
    (limit: number | null) => loadBodyBytes(endpointId, captureId, limit),
    [endpointId, captureId],
  );

  const downloadRaw = useCallback(() => {
    setDownloadFailed(false);
    void fetchCaptureBody(endpointId, captureId, { download: true }).then(
      (blob) => {
        downloadBlob(blob, `${captureId}.${RAW_FILE_EXTENSION}`);
      },
      () => {
        setDownloadFailed(true);
      },
    );
  }, [endpointId, captureId]);

  return (
    <>
      {downloadFailed ? <p role="alert">{t('wl.detail.state.downloadFailed')}</p> : null}
      <CaptureDetail
        {...rest}
        state={stateOf(query.data, query.error)}
        capture={query.data ?? null}
        loadBody={loadBody}
        onDownloadRaw={downloadRaw}
      />
    </>
  );
}
