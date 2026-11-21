import { describe, expect, it } from 'vitest';
import { rawBodyBlob, suggestedFileName } from './download';
import { binaryBody, makeCapture } from './testFixtures';

describe('rawBodyBlob', () => {
  it('is application/octet-stream and holds the exact bytes, never the captured content type', async () => {
    const blob = rawBodyBlob(makeCapture({ content_type: 'text/html', body: binaryBody([0, 255, 60]) }));
    expect(blob.type).toBe('application/octet-stream');
    expect(new Uint8Array(await blob.arrayBuffer())).toEqual(new Uint8Array([0, 255, 60]));
  });
});

describe('suggestedFileName', () => {
  it('builds method, short id and timestamp with a kind extension', () => {
    expect(suggestedFileName(makeCapture(), 'json')).toBe('post-0193a1c2-2026-11-21T10_15_30.123Z.json');
    expect(suggestedFileName(makeCapture(), 'har')).toMatch(/\.har$/);
    expect(suggestedFileName(makeCapture(), 'body')).toMatch(/\.bin$/);
  });

  it('replaces every character outside the safe set', () => {
    const name = suggestedFileName(
      makeCapture({ method: '../GE T/‮😀', id: 'a/b\\c:d*e?f', received_at: 'x y"z<>|' }),
      'json',
    );
    expect(name).toMatch(/^[A-Za-z0-9._-]+$/);
    expect(name).not.toContain('/');
  });
});
