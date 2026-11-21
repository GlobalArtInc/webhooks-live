import { http, type HttpHandler } from 'msw';
import type { Rule, RuleInput, RuleList, RuleOrderInput } from '../../api/rulesTypes';
import { emptyResponse, errorResponse, jsonResponse, readJsonBody } from './responses';

interface EndpointPath {
  endpointId: string;
}

interface RulePath extends EndpointPath {
  ruleId: string;
}

const RULES = '/api/v1/endpoints/:endpointId/rules';
const RULE_ORDER = '/api/v1/endpoints/:endpointId/rules/order';
const RULE = '/api/v1/endpoints/:endpointId/rules/:ruleId';
const EMPTY_VERSION = 1;

const rulesByEndpoint = new Map<string, RuleList>();

export function seedRules(endpointId: string, items: Rule[]): void {
  rulesByEndpoint.set(endpointId, { items: [...items], version: EMPTY_VERSION });
}

export function resetRulesStore(): void {
  rulesByEndpoint.clear();
}

function listOf(endpointId: string): RuleList {
  return rulesByEndpoint.get(endpointId) ?? { items: [], version: EMPTY_VERSION };
}

function replaceItems(endpointId: string, items: Rule[]): RuleList {
  const next: RuleList = { items, version: listOf(endpointId).version + 1 };
  rulesByEndpoint.set(endpointId, next);
  return next;
}

function isPermutation(ids: string[], items: Rule[]): boolean {
  return (
    ids.length === items.length &&
    new Set(ids).size === items.length &&
    ids.every((id) => items.some((rule) => rule.id === id))
  );
}

function orderInvalid(): Response {
  return errorResponse(422, 'validation_failed', 'ids must list every rule exactly once.', [
    { field: 'ids', code: 'invalid_format', message: 'must list every rule exactly once' },
  ]);
}

export const ruleHandlers: HttpHandler[] = [
  http.get<EndpointPath>(RULES, ({ params }) => jsonResponse(listOf(params.endpointId))),

  http.put<EndpointPath>(RULE_ORDER, async ({ params, request }) => {
    const body = await readJsonBody<RuleOrderInput>(request);
    const current = listOf(params.endpointId);
    if (body === null || !Array.isArray(body.ids) || !isPermutation(body.ids, current.items)) {
      return orderInvalid();
    }
    const byId = new Map<string, Rule>(current.items.map((rule) => [rule.id, rule] as const));
    const items = body.ids.flatMap((id) => {
      const rule = byId.get(id);
      return rule === undefined ? [] : [rule];
    });
    return jsonResponse(replaceItems(params.endpointId, items));
  }),

  http.patch<RulePath>(RULE, async ({ params, request }) => {
    const current = listOf(params.endpointId);
    const existing = current.items.find((rule) => rule.id === params.ruleId);
    if (existing === undefined) {
      return errorResponse(404, 'rule_not_found', 'Rule not found.');
    }
    const input = await readJsonBody<RuleInput>(request);
    if (input === null) {
      return errorResponse(400, 'invalid_json', 'Body must be a JSON object.');
    }
    const updated: Rule = { ...existing, ...input, updated_at: new Date().toISOString() };
    replaceItems(
      params.endpointId,
      current.items.map((rule) => (rule.id === updated.id ? updated : rule)),
    );
    return jsonResponse(updated);
  }),

  http.delete<RulePath>(RULE, ({ params }) => {
    const current = listOf(params.endpointId);
    if (!current.items.some((rule) => rule.id === params.ruleId)) {
      return errorResponse(404, 'rule_not_found', 'Rule not found.');
    }
    replaceItems(
      params.endpointId,
      current.items.filter((rule) => rule.id !== params.ruleId),
    );
    return emptyResponse();
  }),
];
