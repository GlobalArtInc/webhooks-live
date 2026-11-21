import { describe, expect, it } from 'vitest';
import type { MyEndpoint } from '../endpoints/storage';
import {
  ANONYMOUS_ENDPOINT_LIMIT,
  formatInboxScope,
  parseInboxScope,
  queryForKey,
  queryOf,
  resolveScope,
  selectValueOf,
} from './scope';

const FIRST = '0123456789abcdefghjkmnpq';
const SECOND = '0123456789abcdefghjkmnpr';
const FOLDER = 'f0ld3r-1';

function localEndpoint(index: number): MyEndpoint {
  return {
    id: String(index).padStart(24, '0'),
    label: null,
    added: index,
    lastOpened: index,
    lastSeenCaptureId: null,
  };
}

describe('inbox scope', () => {
  it('round-trips the URL forms and drops malformed endpoint ids', () => {
    expect(formatInboxScope(parseInboxScope(undefined))).toBe('all');
    expect(formatInboxScope(parseInboxScope(`endpoints:${FIRST},not-an-id,${FIRST}`))).toBe(`endpoints:${FIRST}`);
    expect(formatInboxScope(parseInboxScope('endpoints'))).toBe('endpoints');
    expect(formatInboxScope(parseInboxScope(`folder:${FOLDER}`))).toBe(`folder:${FOLDER}`);
    expect(parseInboxScope('folder:../x')).toEqual({ kind: 'all' });
  });

  it('reports the select value of a chosen-endpoints scope as one option', () => {
    expect(selectValueOf(parseInboxScope(`endpoints:${FIRST},${SECOND}`))).toBe('endpoints');
    expect(selectValueOf(parseInboxScope(`folder:${FOLDER}`))).toBe(`folder:${FOLDER}`);
  });

  it('narrows an anonymous all-scope to the most recently opened endpoints and drops folders', () => {
    const local = Array.from({ length: ANONYMOUS_ENDPOINT_LIMIT + 3 }, (_value, index) => localEndpoint(index + 1));
    const anonymous = resolveScope(parseInboxScope(undefined), false, local);
    expect(anonymous).toEqual({
      kind: 'endpoints',
      endpointIds: [...local]
        .reverse()
        .slice(0, ANONYMOUS_ENDPOINT_LIMIT)
        .map((entry) => entry.id),
    });
    expect(resolveScope(parseInboxScope(`folder:${FOLDER}`), false, local)).toEqual({ kind: 'all' });
    expect(resolveScope(parseInboxScope(undefined), true, local)).toEqual({ kind: 'all' });
  });

  it('sends no request for an empty chosen-endpoints scope and joins the ids otherwise', () => {
    expect(queryOf(parseInboxScope('endpoints'))).toBeNull();
    expect(queryForKey(`endpoints:${FIRST},${SECOND}`)).toEqual({ endpoints: `${FIRST},${SECOND}` });
    expect(queryForKey(`folder:${FOLDER}`)).toEqual({ folder: FOLDER });
  });
});
