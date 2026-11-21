import { http, type HttpHandler } from 'msw';
import type { Capture } from '../../api/types';
import type { PreviewRequest } from '../../features/templating/types';
import { captureIdFor } from '../../features/endpoint/testFixtures';
import { catalogueFixture } from '../fixtures/templating';
import { renderFake } from '../fixtures/templatingRender';
import { jsonCaptureFixture } from '../fixtures/captures';
import { ENDPOINT_ID } from '../fixtures/endpoint';
import { PUBLIC_URL } from '../fixtures/instance';
import { emptyResponse, errorResponse, jsonResponse, readJsonBody } from './responses';
import { mockStore, storeCaptures } from './store';

export interface TemplatingLog {
  previews: PreviewRequest[];
  variableRequests: (string | null)[];
  sampleRequests: number;
}

const PREVIEW = '/api/v1/endpoints/:endpointId/template/preview';
const VARIABLES = '/api/v1/endpoints/:endpointId/template/variables';
const SAMPLE_TARGET = `/${ENDPOINT_ID}`;

function newCapture(seq: number): Capture {
  return {
    ...jsonCaptureFixture,
    id: captureIdFor(seq),
    seq,
    endpoint_id: ENDPOINT_ID,
    received_at: new Date(Date.parse('2026-11-21T10:00:00.000Z') + seq * 1000).toISOString(),
    path: `/hook/${seq}`,
    query_string: '',
    url: `${PUBLIC_URL}/${ENDPOINT_ID}/hook/${seq}`,
    status_override: null,
    forwards: [],
  };
}

export function addCapture(endpointId: string): Capture {
  const existing = mockStore.captures.get(endpointId) ?? [];
  const seq = existing.reduce((max, capture) => Math.max(max, capture.seq), 0) + 1;
  const capture = newCapture(seq);
  storeCaptures(endpointId, [...existing, capture]);
  return capture;
}

export function templatingHandlers(): { handlers: HttpHandler[]; log: TemplatingLog } {
  const log: TemplatingLog = { previews: [], variableRequests: [], sampleRequests: 0 };
  const handlers: HttpHandler[] = [
    http.get<{ endpointId: string }>(VARIABLES, ({ request }) => {
      log.variableRequests.push(new URL(request.url).searchParams.get('capture_id'));
      return jsonResponse(catalogueFixture);
    }),
    http.post<{ endpointId: string }>(PREVIEW, async ({ params, request }) => {
      const input = await readJsonBody<PreviewRequest>(request);
      if (input === null) {
        return errorResponse(400, 'invalid_json', 'Request body is not valid JSON.');
      }
      log.previews.push(input);
      const captures = mockStore.captures.get(params.endpointId) ?? [];
      const capture = input.capture_id === null ? null : captures.find((item) => item.id === input.capture_id);
      if (capture === undefined) {
        return errorResponse(404, 'capture_not_found', 'Capture not found.');
      }
      return jsonResponse(renderFake(input, capture));
    }),
    http.post(SAMPLE_TARGET, () => {
      log.sampleRequests += 1;
      addCapture(ENDPOINT_ID);
      return emptyResponse(202);
    }),
  ];
  return { handlers, log };
}
