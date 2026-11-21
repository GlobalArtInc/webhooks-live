import type { CaptureSummary } from '../../api/types';

export function resolveImplicitId(
  items: readonly CaptureSummary[],
  pinnedId: string | null,
  following: boolean,
): string | null {
  const newest = items[0]?.id ?? null;
  if (following || pinnedId === null) {
    return newest;
  }
  return items.some((item) => item.id === pinnedId) ? pinnedId : newest;
}
