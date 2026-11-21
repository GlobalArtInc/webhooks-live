import { describe, expect, it } from 'vitest';
import { binaryCaptureFixture } from '../../test/fixtures/captures';
import { DEFAULT_NORMALISE_OPTIONS } from './normalize';
import { compareCaptures, isIdentical } from './summary';
import { leftCaptureFixture, rightCaptureFixture } from './testFixtures';

describe('compareCaptures', () => {
  it('counts added, removed, changed and same header rows, and hidden volatile headers', () => {
    const summary = compareCaptures(leftCaptureFixture, rightCaptureFixture, DEFAULT_NORMALISE_OPTIONS, true);

    expect(summary.headers).toEqual({ added: 2, removed: 0, changed: 1, same: 2, hidden: 2 });
  });

  it('counts volatile headers as changed rows when they are not hidden', () => {
    const summary = compareCaptures(
      leftCaptureFixture,
      rightCaptureFixture,
      { ...DEFAULT_NORMALISE_OPTIONS, hideVolatile: false },
      true,
    );

    expect(summary.headers).toEqual({ added: 2, removed: 0, changed: 3, same: 2, hidden: 0 });
  });

  it('counts a header that only the left capture has as removed', () => {
    const summary = compareCaptures(rightCaptureFixture, leftCaptureFixture, DEFAULT_NORMALISE_OPTIONS, true);

    expect(summary.headers).toEqual({ added: 0, removed: 2, changed: 1, same: 2, hidden: 2 });
  });

  it('reports the request fields that differ', () => {
    const summary = compareCaptures(leftCaptureFixture, rightCaptureFixture, DEFAULT_NORMALISE_OPTIONS, true);

    expect(summary.fields).toEqual({ method: true, path: true, query: false });
  });

  it('treats JSON bodies that differ only in key order as the same body', () => {
    const reordered = {
      ...leftCaptureFixture,
      body: { encoding: 'utf8' as const, data: '{"b":1,"a":2}', truncated: false },
    };
    const original = {
      ...leftCaptureFixture,
      body: { encoding: 'utf8' as const, data: '{"a":2,"b":1}', truncated: false },
    };

    expect(compareCaptures(reordered, original, DEFAULT_NORMALISE_OPTIONS, true).body).toBe('same');
  });

  it('reports the body as not compared when bodies are excluded', () => {
    expect(compareCaptures(leftCaptureFixture, rightCaptureFixture, DEFAULT_NORMALISE_OPTIONS, false).body).toBe(
      'not-compared',
    );
  });

  it('compares binary bodies by their bytes', () => {
    const other = { ...binaryCaptureFixture, body: { ...binaryCaptureFixture.body, data: 'AAAAAA==' } };

    expect(
      compareCaptures(binaryCaptureFixture, { ...binaryCaptureFixture }, DEFAULT_NORMALISE_OPTIONS, true).body,
    ).toBe('same');
    expect(compareCaptures(binaryCaptureFixture, other, DEFAULT_NORMALISE_OPTIONS, true).body).toBe('different');
  });

  it('is identical only when every compared part matches', () => {
    expect(isIdentical(compareCaptures(leftCaptureFixture, leftCaptureFixture, DEFAULT_NORMALISE_OPTIONS, true))).toBe(
      true,
    );
    expect(isIdentical(compareCaptures(leftCaptureFixture, rightCaptureFixture, DEFAULT_NORMALISE_OPTIONS, true))).toBe(
      false,
    );
  });
});
