import { describe, expect, it } from 'vitest';
import { createKeyValueRow } from '../../components/KeyValueEditor/rows';
import { jsonCaptureFixture } from '../../test/fixtures/captures';
import { sentRequestFixture } from '../../test/fixtures/composer';
import { ENDPOINT_ID } from '../../test/fixtures/endpoint';
import {
  buildSendRequest,
  contentTypeOf,
  emptyDraft,
  languageOf,
  parseTimeout,
  redactedRowIds,
  seedFromCapture,
  seedFromSent,
  withContentType,
} from './draft';

describe('composer draft', () => {
  it('replays a capture with its path, query, body and only the headers that may be sent again', () => {
    const seed = seedFromCapture(jsonCaptureFixture);

    expect(seed.draft.url).toBe('/404/orders?x=1');
    expect(seed.draft.body).toEqual({ encoding: 'utf8', data: '{"order":42}', truncated: false });
    expect(seed.draft.headers.map((row) => row.name)).toEqual(['Content-Type']);
    expect(seed.banner).toEqual({ method: 'POST', target: '/404/orders?x=1' });
    expect(seed.sourceCaptureId).toBe(jsonCaptureFixture.id);
  });

  it('sends the captured body unchanged and leaves out an empty body and the endpoint when none is given', () => {
    const replayed = buildSendRequest(seedFromCapture(jsonCaptureFixture).draft, {
      endpointId: null,
      sourceCaptureId: jsonCaptureFixture.id,
    });
    expect(replayed.body).toEqual({ encoding: 'utf8', data: '{"order":42}' });
    expect(replayed.source_capture_id).toBe(jsonCaptureFixture.id);
    expect(replayed).not.toHaveProperty('endpoint_id');

    const blank = buildSendRequest(emptyDraft(), { endpointId: ENDPOINT_ID, sourceCaptureId: null });
    expect(blank.body).toBeNull();
    expect(blank.endpoint_id).toBe(ENDPOINT_ID);
  });

  it('keeps a redacted header value from a history entry visible so the send can be blocked on it', () => {
    const seed = seedFromSent(sentRequestFixture);
    const ids = redactedRowIds(seed.draft.headers);

    expect(ids).toHaveLength(1);
    expect(seed.draft.headers.find((row) => row.id === ids[0])?.name).toBe('authorization');
  });

  it('clamps the timeout and rejects values below one millisecond', () => {
    expect(parseTimeout('250')).toBe(250);
    expect(parseTimeout('99999')).toBe(15000);
    expect(parseTimeout('0')).toBeNull();
    expect(parseTimeout('')).toBeNull();
  });

  it('sets, replaces and removes the content type and picks the editor language from it', () => {
    const rows = withContentType([createKeyValueRow('x-trace', '1')], 'application/json');
    expect(contentTypeOf(rows)).toBe('application/json');
    expect(languageOf(contentTypeOf(rows))).toBe('json');

    const replaced = withContentType(rows, 'text/html');
    expect(contentTypeOf(replaced)).toBe('text/html');
    expect(languageOf('text/html')).toBe('html');

    expect(withContentType(replaced, '').map((row) => row.name)).toEqual(['x-trace']);
  });
});
