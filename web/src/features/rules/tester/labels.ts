import type { TFunction } from 'i18next';
import type { TraceStatus } from './types';

export function statusLabel(status: TraceStatus, t: TFunction): string {
  switch (status) {
    case 'matched':
      return t('wl.tester.status.matched');
    case 'notMatched':
      return t('wl.tester.status.notMatched');
    case 'ran':
      return t('wl.tester.status.ran');
    case 'failed':
      return t('wl.tester.status.failed');
    case 'simulated':
      return t('wl.tester.status.simulated');
    case 'skipped':
      return t('wl.tester.status.skipped');
    case 'deferred':
      return t('wl.tester.status.deferred');
    case 'notReached':
      return t('wl.tester.status.notReached');
  }
}

export function statusGlyph(status: TraceStatus): string {
  switch (status) {
    case 'matched':
    case 'ran':
      return '✓';
    case 'failed':
      return '✕';
    case 'simulated':
      return '➤';
    case 'skipped':
      return '↷';
    case 'notMatched':
      return '−';
    case 'deferred':
    case 'notReached':
      return '·';
  }
}

export function humanizeKind(kind: string): string {
  const spaced = kind.replace(/_/g, ' ').trim();
  return spaced === '' ? kind : `${spaced.charAt(0).toUpperCase()}${spaced.slice(1)}`;
}

export function stepTitle(kind: string | null, titles: Readonly<Record<string, string>> | undefined): string {
  if (kind === null || kind === '') {
    return '';
  }
  return titles?.[kind] ?? humanizeKind(kind);
}
