import styles from './BinaryView.module.css';
import { formatSize } from './bytes';
import { sha256Hex } from './hash';
import { HexView } from './HexView';
import type { Bytes } from './types';
import { useAsyncValue } from './useAsyncValue';

interface BinaryViewProps {
  bytes: Bytes;
  contentType: string;
  size: number;
  complete: boolean;
}

interface DigestProps {
  bytes: Bytes;
  complete: boolean;
}

function Digest({ bytes, complete }: DigestProps) {
  const digest = useAsyncValue(complete ? bytes : null, () => sha256Hex(bytes));

  if (!complete) {
    return <span>Available after the full body is loaded</span>;
  }
  switch (digest.status) {
    case 'done':
      return <span className={styles.digest}>{digest.value}</span>;
    case 'failed':
      return <span>Unavailable: {digest.message}</span>;
    default:
      return <span>Computing</span>;
  }
}

export function BinaryView({ bytes, contentType, size, complete }: BinaryViewProps) {
  return (
    <div className={styles.view}>
      <dl className={styles.facts}>
        <dt>Type</dt>
        <dd>{contentType === '' ? 'No content type, binary data' : `${contentType}, binary data`}</dd>
        <dt>Size</dt>
        <dd>{formatSize(size)}</dd>
        <dt>SHA-256</dt>
        <dd>
          <Digest bytes={bytes} complete={complete} />
        </dd>
      </dl>
      <HexView bytes={bytes} />
    </div>
  );
}
