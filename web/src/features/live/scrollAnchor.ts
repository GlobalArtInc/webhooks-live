export const AT_TOP_THRESHOLD_PX = 2;

export interface Identified {
  id: string;
}

export function isAtTop(scrollTop: number): boolean {
  return scrollTop <= AT_TOP_THRESHOLD_PX;
}

export function countAbove(items: readonly Identified[], anchorId: string | null): number {
  if (anchorId === null) {
    return 0;
  }
  return Math.max(
    0,
    items.findIndex((item) => item.id === anchorId),
  );
}
