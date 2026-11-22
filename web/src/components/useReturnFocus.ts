import { useCallback, useRef } from 'react';

export interface ReturnFocus {
  onOpenAutoFocus: () => void;
  onCloseAutoFocus: (event: Event) => void;
}

export function useReturnFocus(): ReturnFocus {
  const openerRef = useRef<HTMLElement | null>(null);

  const onOpenAutoFocus = useCallback(() => {
    const active = document.activeElement;
    openerRef.current = active instanceof HTMLElement && active !== document.body ? active : null;
  }, []);

  const onCloseAutoFocus = useCallback((event: Event) => {
    const opener = openerRef.current;
    openerRef.current = null;
    if (opener !== null && opener.isConnected) {
      event.preventDefault();
      opener.focus();
    }
  }, []);

  return { onOpenAutoFocus, onCloseAutoFocus };
}
