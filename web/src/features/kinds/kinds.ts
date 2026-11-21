import type { CaptureKind, CaptureSummary } from '../../api/types';

export type BadgeKind = 'email' | 'dns';

const KINDS: readonly CaptureKind[] = ['http', 'email', 'dns', 'schedule'];

export function captureKindOf(summary: Pick<CaptureSummary, 'kind'>): CaptureKind {
  return KINDS.find((kind) => kind === summary.kind) ?? 'http';
}

export function badgeKindOf(kind: CaptureKind): BadgeKind | null {
  return kind === 'email' || kind === 'dns' ? kind : null;
}
