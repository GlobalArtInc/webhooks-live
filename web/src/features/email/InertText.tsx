import { useMemo } from 'react';
import styles from './EmailDetail.module.css';
import { segmentInert } from './inert';

export interface InertTextProps {
  value: string;
}

export function InertText({ value }: InertTextProps) {
  const segments = useMemo(() => segmentInert(value), [value]);
  return (
    <bdi className={styles.inert}>
      {segments.map((segment, index) =>
        segment.escaped ? (
          <span key={index} className={styles.escape}>
            {segment.text}
          </span>
        ) : (
          segment.text
        ),
      )}
    </bdi>
  );
}
