import { describe, expect, it } from 'vitest';
import { formatByteSize, formatRelativeTime, formatTimestamp } from './format';

const NOW = Date.parse('2026-11-21T10:00:00.000Z');
const RECEIVED = '2026-11-21T09:59:30.000Z';

describe('formatByteSize', () => {
  it('uses plain bytes below one kilobyte and scales above it', () => {
    expect(formatByteSize(4, 'en')).toBe('4B');
    expect(formatByteSize(1500, 'en')).toBe('1.5kB');
    expect(formatByteSize(5_000_000, 'en')).toBe('5MB');
  });
});

describe('formatRelativeTime', () => {
  it('describes seconds, minutes and days relative to now', () => {
    expect(formatRelativeTime('2026-11-21T09:59:30.000Z', NOW, 'en')).toBe('30 seconds ago');
    expect(formatRelativeTime('2026-11-21T09:00:00.000Z', NOW, 'en')).toBe('1 hour ago');
    expect(formatRelativeTime('2026-11-20T10:00:00.000Z', NOW, 'en')).toBe('yesterday');
  });
});

describe('formatTimestamp', () => {
  it('keeps the relative text in the relative mode', () => {
    expect(formatTimestamp(RECEIVED, NOW, 'en', 'relative')).toBe('30 seconds ago');
  });

  it('names the UTC zone in the UTC mode', () => {
    expect(formatTimestamp(RECEIVED, NOW, 'en', 'utc')).toMatch(/UTC$/);
    expect(formatTimestamp(RECEIVED, NOW, 'en', 'utc')).toContain('9:59:30');
  });
});
