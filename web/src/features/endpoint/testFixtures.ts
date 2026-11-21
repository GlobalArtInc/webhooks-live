import type { Capture, CaptureSummary } from '../../api/types';
import { jsonCaptureFixture } from '../../test/fixtures/captures';
import { ENDPOINT_ID } from '../../test/fixtures/endpoint';
import { PUBLIC_URL } from '../../test/fixtures/instance';
import { summaryOf } from '../../test/msw/store';

const BASE_TIME_MS = Date.parse('2026-11-21T10:00:00.000Z');
const SECOND_MS = 1000;
const SUMMARY_CACHE_SIZE = 10;

export function captureIdFor(seq: number): string {
  return `00000000-0000-4000-8000-${seq.toString(16).padStart(12, '0')}`;
}

export function captureSeries(count: number): Capture[] {
  return Array.from({ length: count }, (_value, index) => {
    const seq = index + 1;
    return {
      ...jsonCaptureFixture,
      id: captureIdFor(seq),
      seq,
      endpoint_id: ENDPOINT_ID,
      received_at: new Date(BASE_TIME_MS + seq * SECOND_MS).toISOString(),
      url: `${PUBLIC_URL}/${ENDPOINT_ID}/hook/${seq}`,
      path: `/hook/${seq}`,
      query_string: '',
      body_size: seq,
      status_override: null,
      forward_state: null,
      forwards: [],
    };
  });
}

const SERIES = captureSeries(SUMMARY_CACHE_SIZE);

export function summaryAt(seq: number): CaptureSummary {
  const capture = SERIES[seq - 1];
  if (capture === undefined) {
    throw new Error(`no fixture capture with seq ${seq}`);
  }
  return summaryOf(capture);
}
