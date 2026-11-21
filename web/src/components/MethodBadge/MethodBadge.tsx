import styles from './MethodBadge.module.css';

interface MethodBadgeProps {
  method: string;
}

const KNOWN_METHOD_KEYS = ['get', 'post', 'put', 'patch', 'delete', 'options', 'head'];

function methodKey(method: string): string {
  const key = method.toLowerCase();
  return KNOWN_METHOD_KEYS.includes(key) ? key : 'other';
}

export function MethodBadge({ method }: MethodBadgeProps) {
  const key = methodKey(method);

  return (
    <span
      className={styles.badge}
      style={{ color: `var(--method-${key}-text)`, background: `var(--method-${key}-bg)` }}
    >
      {method}
    </span>
  );
}
