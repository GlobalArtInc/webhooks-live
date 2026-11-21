export type MoveControl = 'up' | 'down' | 'top';

export function targetIndex(index: number, control: MoveControl, total: number): number {
  if (control === 'top') {
    return 0;
  }
  if (control === 'up') {
    return Math.max(0, index - 1);
  }
  return Math.min(total - 1, index + 1);
}

export function moveItem<T>(items: readonly T[], from: number, to: number): T[] {
  const next = [...items];
  const [moved] = next.splice(from, 1);
  if (moved === undefined) {
    return [...items];
  }
  next.splice(to, 0, moved);
  return next;
}
