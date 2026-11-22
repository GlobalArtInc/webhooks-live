import { useEffect, useRef, useState, type FocusEvent, type ReactNode } from 'react';
import styles from './RailTip.module.css';

export interface RailTipProps {
  label: string;
  children: ReactNode;
}

interface Anchor {
  top: number;
  left: number;
}

export function RailTip({ label, children }: RailTipProps) {
  const hostRef = useRef<HTMLSpanElement>(null);
  const [anchor, setAnchor] = useState<Anchor | null>(null);

  function show() {
    const box = hostRef.current?.getBoundingClientRect();
    if (box !== undefined) {
      setAnchor({ top: box.top + box.height / 2, left: box.right });
    }
  }

  function showOnKeyboardFocus(event: FocusEvent<HTMLSpanElement>) {
    if (event.target.matches(':focus-visible')) {
      show();
    }
  }

  useEffect(() => {
    if (anchor === null) {
      return undefined;
    }
    function dismiss(event: KeyboardEvent) {
      if (event.key === 'Escape') {
        setAnchor(null);
      }
    }
    document.addEventListener('keydown', dismiss);
    return () => document.removeEventListener('keydown', dismiss);
  }, [anchor]);

  return (
    <span
      ref={hostRef}
      className={styles.host}
      onMouseEnter={show}
      onMouseLeave={() => setAnchor(null)}
      onFocus={showOnKeyboardFocus}
      onBlur={() => setAnchor(null)}
    >
      {children}
      {anchor === null ? null : (
        <span className={styles.tip} style={{ top: anchor.top, left: anchor.left }} aria-hidden="true">
          <span className={styles.bubble}>{label}</span>
        </span>
      )}
    </span>
  );
}
