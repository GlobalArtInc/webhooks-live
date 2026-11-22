import styles from './MiddleTruncate.module.css';

const DEFAULT_TAIL_LENGTH = 10;

export interface MiddleTruncateProps {
  text: string;
  prefix?: string;
  tailLength?: number;
}

export function MiddleTruncate({ text, prefix = '', tailLength = DEFAULT_TAIL_LENGTH }: MiddleTruncateProps) {
  const split = Math.max(0, text.length - tailLength);
  return (
    <span className={styles.root} title={`${prefix}${text}`}>
      {prefix === '' ? null : <span className={styles.prefix}>{prefix}</span>}
      <span className={styles.head}>{text.slice(0, split)}</span>
      <span className={styles.tail}>{text.slice(split)}</span>
    </span>
  );
}
