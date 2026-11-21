import type { Capture } from '../../../api/types';
import { simulateDryRun } from './fixtureEngine';
import { TESTER_CAPTURES, TESTER_NEW_CAPTURE_ID, TESTER_SAVED_RULES } from './fixtures';
import { parseRuleRun } from './parse';
import { realRunTarget } from './transport';
import type { TesterTransport } from './types';

function find(captureId: string): Capture {
  const capture = TESTER_CAPTURES.find((candidate) => candidate.id === captureId);
  if (capture === undefined) {
    throw new Error(`Unknown fixture capture ${captureId}`);
  }
  return capture;
}

export const fixtureTesterTransport: TesterTransport = {
  listCaptures: () => Promise.resolve(TESTER_CAPTURES),
  getCapture: (_endpointId, captureId) => Promise.resolve(find(captureId)),
  dryRun: (_endpointId, request) =>
    Promise.resolve(parseRuleRun(simulateDryRun(request.rules, find(request.capture_id), { extended: true }))),
  sendReal: (_endpointId, endpointUrl, capture) =>
    Promise.resolve({
      target: realRunTarget(endpointUrl, capture),
      send: {
        id: TESTER_NEW_CAPTURE_ID,
        status: 200,
        headers: [],
        body: null,
        duration_ms: 12,
        final_url: null,
        error: null,
        capture_id: TESTER_NEW_CAPTURE_ID,
      },
    }),
  listRules: () => Promise.resolve(TESTER_SAVED_RULES),
  sendSample: () => Promise.resolve(TESTER_CAPTURES[0]?.id ?? null),
};
