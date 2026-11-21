import { CopyButton } from '../../components/CopyButton/CopyButton';
import styles from './CodeBlock.module.css';

export interface CodeBlockProps {
  text: string;
  label: string;
  copyLabel?: string;
}

export function CodeBlock({ text, label, copyLabel }: CodeBlockProps) {
  return (
    <div className={styles.wrap}>
      <pre className={styles.code} role="region" aria-label={label} tabIndex={0}>
        <code>{text}</code>
      </pre>
      {copyLabel === undefined ? null : <CopyButton value={text} label={copyLabel} />}
    </div>
  );
}
