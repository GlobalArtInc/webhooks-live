import { describe, expect, it } from 'vitest';
import { jsonCaptureFixture } from '../../test/fixtures/captures';
import { ENDPOINT_ID } from '../../test/fixtures/endpoint';
import { replayCaptureInComposer, validateComposeSearch } from './search';

describe('composer route search', () => {
  it('keeps well-formed endpoint and capture ids and drops anything else', () => {
    expect(validateComposeSearch({ endpoint: ENDPOINT_ID, capture: jsonCaptureFixture.id })).toEqual({
      endpoint: ENDPOINT_ID,
      capture: jsonCaptureFixture.id,
    });
    expect(validateComposeSearch({ endpoint: '../etc', capture: 42 })).toEqual({
      endpoint: undefined,
      capture: undefined,
    });
  });

  it('builds the navigation that opens a capture replay', () => {
    expect(replayCaptureInComposer(jsonCaptureFixture.id, ENDPOINT_ID)).toEqual({
      to: '/send',
      search: { endpoint: ENDPOINT_ID, capture: jsonCaptureFixture.id },
    });
  });
});
