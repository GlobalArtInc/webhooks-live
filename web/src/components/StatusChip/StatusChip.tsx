import styles from './StatusChip.module.css';

export type StatusChipKind = 'answered' | 'forward' | 'override';

type StatusTone = 'success' | 'info' | 'warning' | 'danger' | 'neutral' | 'override';

interface StatusChipProps {
  code: number;
  kind: StatusChipKind;
  label: string;
}

const SUCCESS_MIN = 200;
const REDIRECT_MIN = 300;
const CLIENT_ERROR_MIN = 400;
const SERVER_ERROR_MIN = 500;
const SERVER_ERROR_MAX = 599;

function toneOf(code: number, kind: StatusChipKind): StatusTone {
  if (kind === 'override') {
    return 'override';
  }
  if (code >= SUCCESS_MIN && code < REDIRECT_MIN) {
    return 'success';
  }
  if (code >= REDIRECT_MIN && code < CLIENT_ERROR_MIN) {
    return 'info';
  }
  if (code >= CLIENT_ERROR_MIN && code < SERVER_ERROR_MIN) {
    return 'warning';
  }
  if (code >= SERVER_ERROR_MIN && code <= SERVER_ERROR_MAX) {
    return 'danger';
  }
  return 'neutral';
}

export function StatusChip({ code, kind, label }: StatusChipProps) {
  const tone = toneOf(code, kind);
  return <span className={`${styles.chip} ${styles[tone] ?? ''}`}>{label}</span>;
}
