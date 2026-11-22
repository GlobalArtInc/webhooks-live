import { useCallback, useRef, type RefObject } from 'react';

export interface ReturnFocus {
  onOpenAutoFocus: () => void;
  onCloseAutoFocus: (event: Event) => void;
}

function currentOf(ref: RefObject<HTMLElement | null> | undefined): HTMLElement | null {
  return ref === undefined ? null : ref.current;
}

export function useReturnFocus(fallback?: RefObject<HTMLElement | null>): ReturnFocus {
  const openerRef = useRef<HTMLElement | null>(null);

  const onOpenAutoFocus = useCallback(() => {
    const active = document.activeElement;
    openerRef.current = active instanceof HTMLElement && active !== document.body ? active : null;
  }, []);

  const onCloseAutoFocus = useCallback(
    (event: Event) => {
      const opener = openerRef.current;
      openerRef.current = null;
      const target = opener !== null && opener.isConnected ? opener : currentOf(fallback);
      if (target !== null && target.isConnected) {
        event.preventDefault();
        target.focus();
      }
    },
    [fallback],
  );

  return { onOpenAutoFocus, onCloseAutoFocus };
}
