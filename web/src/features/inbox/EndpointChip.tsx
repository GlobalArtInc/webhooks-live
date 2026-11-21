import styles from './EndpointChip.module.css';

export interface EndpointChipProps {
  endpointId: string;
  label: string;
}

export function EndpointChip({ endpointId, label }: EndpointChipProps) {
  return (
    <span className={styles.chip} title={endpointId}>
      {label}
    </span>
  );
}
