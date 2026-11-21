import { endpointPath, getCapture, listCaptures, requestJson, sendRequest } from '../../../api/client';
import { listRules } from '../../../api/rules';
import type { Capture } from '../../../api/types';
import { buildSendRequest, redactedRowIds, seedFromCapture } from '../../composer/draft';
import { CAPTURE_LIST_LIMIT } from './constants';
import { sendSampleRequest } from '../../templating/api';
import { parseRuleRun } from './parse';
import type { DryRunRequest, RealRunResult, RuleRunResult, TesterTransport } from './types';

function dryRunPath(endpointId: string): string {
  return `${endpointPath(endpointId)}/rules/dry-run`;
}

export function realRunTarget(endpointUrl: string, capture: Capture): string {
  const base = endpointUrl.replace(/\/+$/, '');
  const query = capture.query_string === '' ? '' : `?${capture.query_string}`;
  return `${base}${capture.path}${query}`;
}

async function dryRun(endpointId: string, request: DryRunRequest, signal: AbortSignal): Promise<RuleRunResult> {
  const payload = await requestJson<unknown>(dryRunPath(endpointId), { method: 'POST', body: request, signal });
  return parseRuleRun(payload);
}

async function sendReal(endpointId: string, endpointUrl: string, capture: Capture): Promise<RealRunResult> {
  const seed = seedFromCapture(capture);
  const target = realRunTarget(endpointUrl, capture);
  const skipped = new Set(redactedRowIds(seed.draft.headers));
  const draft = {
    ...seed.draft,
    url: target,
    headers: seed.draft.headers.filter((row) => !skipped.has(row.id)),
  };
  const send = await sendRequest(buildSendRequest(draft, { endpointId, sourceCaptureId: capture.id }));
  return { send, target };
}

export const httpTesterTransport: TesterTransport = {
  listCaptures: async (endpointId) => {
    const page = await listCaptures(endpointId, { limit: CAPTURE_LIST_LIMIT, order: 'desc' });
    return page.items;
  },
  getCapture: (endpointId, captureId) => getCapture(endpointId, captureId),
  dryRun,
  sendReal,
  listRules: async (endpointId) => (await listRules(endpointId)).items,
  sendSample: async (endpointId, endpointUrl) => {
    await sendSampleRequest(endpointUrl);
    const page = await listCaptures(endpointId, { limit: 1, order: 'desc' });
    return page.items[0]?.id ?? null;
  },
};
