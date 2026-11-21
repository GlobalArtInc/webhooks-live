import { useBlocker, type RegisteredRouter } from '@tanstack/react-router';
import { useCallback } from 'react';

interface NavigationPoint {
  pathname: string;
}

interface BlockedNavigation {
  current: NavigationPoint;
  next: NavigationPoint;
}

export type DirtyBlocker = ReturnType<typeof useBlocker<RegisteredRouter, true>>;

export function useDirtyGuard(dirty: boolean): DirtyBlocker {
  const shouldBlock = useCallback(
    ({ current, next }: BlockedNavigation) => dirty && current.pathname !== next.pathname,
    [dirty],
  );
  return useBlocker({ shouldBlockFn: shouldBlock, enableBeforeUnload: dirty, withResolver: true });
}
