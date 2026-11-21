import { useMemo } from 'react';
import { formatHex, formatSize } from './bytes';
import { HEX_BYTES } from './constants';
import styles from './HexView.module.css';

interface HexViewProps {
  bytes: Uint8Array;
  limit?: number;
}

export function HexView({ bytes, limit = HEX_BYTES }: HexViewProps) {
  const dump = useMemo(() => formatHex(bytes, limit), [bytes, limit]);
  const shown = Math.min(bytes.length, limit);

  return (
    <figure className={styles.figure}>
      <figcaption className={styles.caption}>Hex view of the first {formatSize(shown)}</figcaption>
      <pre className={styles.dump} tabIndex={0} aria-label="Hex dump">
        {dump}
      </pre>
    </figure>
  );
}
