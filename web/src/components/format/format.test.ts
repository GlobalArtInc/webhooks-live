import { describe, expect, it } from 'vitest';
import { formatByteSize, formatElapsedTime, formatRelativeTime, formatTimestamp } from './format';

const NOW = Date.parse('2026-11-21T10:00:00.000Z');
const RECEIVED = '2026-11-21T09:59:30.000Z';

describe('formatByteSize', () => {
  it('uses plain bytes below one kibibyte and binary units above it', () => {
    expect(formatByteSize(4, 'en')).toBe('4 B');
    expect(formatByteSize(1023, 'en')).toBe('1,023 B');
    expect(formatByteSize(2048, 'en')).toBe('2.0 KiB');
    expect(formatByteSize(1536, 'en')).toBe('1.5 KiB');
    expect(formatByteSize(1024 * 1024, 'en')).toBe('1.0 MiB');
    expect(formatByteSize(5 * 1024 * 1024 * 1024, 'en')).toBe('5.0 GiB');
  });

  it('moves up a unit instead of rounding to 1024 of the unit below', () => {
    expect(formatByteSize(1024 * 1024 - 1, 'en')).toBe('1.0 MiB');
  });
});

describe('formatRelativeTime', () => {
  it('keeps real future moments such as an expiry in the future', () => {
    expect(formatRelativeTime('2026-11-21T10:00:08.000Z', NOW, 'en')).toBe('in 8 seconds');
  });

  it('describes seconds, minutes and days relative to now', () => {
    expect(formatRelativeTime('2026-11-21T09:59:30.000Z', NOW, 'en')).toBe('30 seconds ago');
    expect(formatRelativeTime('2026-11-21T09:00:00.000Z', NOW, 'en')).toBe('1 hour ago');
    expect(formatRelativeTime('2026-11-20T10:00:00.000Z', NOW, 'en')).toBe('yesterday');
  });
});

describe('formatElapsedTime', () => {
  it('describes the past like the relative formatter', () => {
    expect(formatElapsedTime('2026-11-21T09:59:30.000Z', NOW, 'en')).toBe('30 seconds ago');
  });

  it('shows a timestamp inside the clock skew as now', () => {
    expect(formatElapsedTime('2026-11-21T10:00:08.000Z', NOW, 'en')).toBe('now');
    expect(formatElapsedTime('2026-11-21T10:05:00.000Z', NOW, 'en')).toBe('now');
  });

  it('keeps a timestamp far beyond the skew in the future', () => {
    expect(formatElapsedTime('2026-11-21T10:06:00.000Z', NOW, 'en')).toBe('in 6 minutes');
  });

  it('leaves unparseable input unchanged', () => {
    expect(formatElapsedTime('soon', NOW, 'en')).toBe('soon');
  });
});

describe('formatTimestamp', () => {
  it('keeps the relative text in the relative mode', () => {
    expect(formatTimestamp(RECEIVED, NOW, 'en', 'relative')).toBe('30 seconds ago');
  });

  it('shows an arrival ahead of the browser clock as now in the relative mode', () => {
    expect(formatTimestamp('2026-11-21T10:00:08.000Z', NOW, 'en', 'relative')).toBe('now');
  });

  it('names the UTC zone in the UTC mode', () => {
    expect(formatTimestamp(RECEIVED, NOW, 'en', 'utc')).toMatch(/UTC$/);
    expect(formatTimestamp(RECEIVED, NOW, 'en', 'utc')).toContain('9:59:30');
  });
});
