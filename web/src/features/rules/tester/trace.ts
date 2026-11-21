import type {
  CutReason,
  EntryOutcome,
  ErrorPosition,
  FinalResponse,
  JsonValue,
  RuleRunResult,
  RuleRunStepEntry,
  TesterRule,
  TraceGroup,
  TraceItem,
  TraceModel,
  TraceSelection,
  TraceStatus,
} from './types';

const LINE_COLUMN = /\bline\s+(\d+)(?:\s*,?\s*(?:column|col)\s+(\d+))?/i;
const AT_POSITION = /(?:\bat\s+|\()(\d+):(\d+)\)?/;
const STOP_KIND = 'stop';
const RESPOND_KIND = 'respond';

export function statusOfOutcome(outcome: EntryOutcome): TraceStatus {
  switch (outcome) {
    case 'ok':
      return 'ran';
    case 'failed':
      return 'failed';
    case 'skipped':
      return 'skipped';
    case 'deferred':
      return 'deferred';
    case 'no_op':
      return 'simulated';
  }
}

export function errorPosition(detail: string | null): ErrorPosition | null {
  if (detail === null) {
    return null;
  }
  const lineMatch = LINE_COLUMN.exec(detail);
  if (lineMatch !== null) {
    return { line: Number(lineMatch[1]), column: lineMatch[2] === undefined ? 1 : Number(lineMatch[2]) };
  }
  const atMatch = AT_POSITION.exec(detail);
  return atMatch === null ? null : { line: Number(atMatch[1]), column: Number(atMatch[2]) };
}

export function selectionOf(item: TraceItem): TraceSelection {
  return { itemId: item.id, ruleId: item.ruleId, stepIndex: item.stepIndex };
}

function stepItem(entry: RuleRunStepEntry, group: number, ruleName: string): TraceItem {
  return {
    id: `step-${group}-${entry.step}`,
    type: 'step',
    group,
    ruleId: entry.ruleId,
    ruleName,
    stepIndex: entry.step,
    position: entry.step + 1,
    stepKind: entry.kind,
    stepName: entry.name,
    status: statusOfOutcome(entry.outcome),
    durationMs: entry.durationMs,
    detail: entry.detail,
    output: entry.output,
    conditions: null,
    vars: entry.vars,
    placeholder: false,
  };
}

function placeholderItem(
  group: number,
  ruleName: string,
  ruleId: string | null,
  index: number,
  rule: TesterRule,
): TraceItem {
  return {
    id: `step-${group}-${index}`,
    type: 'step',
    group,
    ruleId,
    ruleName,
    stepIndex: index,
    position: index + 1,
    stepKind: rule.stepKinds[index] ?? null,
    stepName: rule.stepNames[index] ?? null,
    status: 'notReached',
    durationMs: 0,
    detail: null,
    output: null,
    conditions: null,
    vars: null,
    placeholder: true,
  };
}

interface GroupDraft {
  ruleId: string | null;
  ruleName: string;
  matched: boolean | null;
  items: TraceItem[];
}

function numberOf(value: JsonValue | undefined): number | null {
  return typeof value === 'number' && Number.isFinite(value) ? value : null;
}

function textOfJson(value: JsonValue | undefined): string | null {
  return typeof value === 'string' ? value : null;
}

function respondSummary(item: TraceItem | undefined, result: RuleRunResult): FinalResponse {
  const output = item?.output;
  const record = typeof output === 'object' && output !== null && !Array.isArray(output) ? output : null;
  const detail = result.response;
  return {
    status: result.responseStatus,
    groupIndex: item?.group ?? null,
    contentType: detail?.contentType ?? (record === null ? null : textOfJson(record.content_type)),
    headerCount: detail?.headers.length ?? (record === null ? null : numberOf(record.headers)),
    bodyBytes: detail?.bodySize ?? (record === null ? null : numberOf(record.body_bytes)),
    detail,
  };
}

function cutOf(
  group: TraceGroup,
  items: TraceItem[],
  rule: TesterRule | undefined,
  hasLater: boolean,
): CutReason | null {
  if (!hasLater || group.matched !== true) {
    return null;
  }
  const stopped = items.some((item) => item.stepKind === STOP_KIND && item.status === 'ran');
  if (stopped || rule?.stopsOnMatch === true) {
    return 'stop';
  }
  return items.some((item) => item.status === 'failed') ? 'failed' : null;
}

function finishGroup(draft: GroupDraft, index: number, rule: TesterRule | undefined, total: number): TraceGroup {
  const steps = draft.items.filter((item) => item.type === 'step');
  const real = steps.filter((item) => !item.placeholder);
  const failed = steps.find((item) => item.status === 'failed');
  const group: TraceGroup = {
    index,
    ruleId: draft.ruleId,
    ruleName: draft.ruleName,
    matched: draft.matched,
    itemIds: draft.items.map((item) => item.id),
    stepCount: rule?.stepKinds.length ?? real.length,
    ranCount: real.filter((item) => item.status === 'ran' || item.status === 'simulated').length,
    failedPosition: failed?.position ?? null,
    cut: null,
  };
  group.cut = cutOf(group, draft.items, rule, index + 1 < total);
  return group;
}

function withPlaceholders(draft: GroupDraft, index: number, rule: TesterRule | undefined): TraceItem[] {
  if (rule === undefined || draft.matched !== true) {
    return draft.items;
  }
  const seen = draft.items.filter((item) => item.type === 'step').map((item) => item.stepIndex ?? -1);
  const next = seen.length === 0 ? 0 : Math.max(...seen) + 1;
  const extra: TraceItem[] = [];
  for (let position = next; position < rule.stepKinds.length; position += 1) {
    extra.push(placeholderItem(index, draft.ruleName, draft.ruleId, position, rule));
  }
  return [...draft.items, ...extra];
}

export function buildTrace(result: RuleRunResult, rules: readonly TesterRule[]): TraceModel {
  const drafts: GroupDraft[] = [];
  for (const entry of result.entries) {
    if (entry.type === 'rule') {
      const index = drafts.length;
      drafts.push({
        ruleId: entry.ruleId,
        ruleName: entry.ruleName,
        matched: entry.matched,
        items: [
          {
            id: `rule-${index}`,
            type: 'rule',
            group: index,
            ruleId: entry.ruleId,
            ruleName: entry.ruleName,
            stepIndex: null,
            position: null,
            stepKind: null,
            stepName: null,
            status: entry.matched ? 'matched' : 'notMatched',
            durationMs: 0,
            detail: null,
            output: null,
            conditions: entry.conditions,
            vars: null,
            placeholder: false,
          },
        ],
      });
      continue;
    }
    if (drafts.length === 0) {
      drafts.push({ ruleId: entry.ruleId, ruleName: '', matched: null, items: [] });
    }
    const current = drafts[drafts.length - 1];
    if (current !== undefined) {
      current.items.push(stepItem(entry, drafts.length - 1, current.ruleName));
    }
  }
  const total = Math.max(rules.length, drafts.length);
  const groups: TraceGroup[] = [];
  const items: TraceItem[] = [];
  drafts.forEach((draft, index) => {
    const rule = rules[index];
    const full = { ...draft, items: withPlaceholders(draft, index, rule) };
    items.push(...full.items);
    groups.push(finishGroup(full, index, rule, total));
  });
  for (let index = drafts.length; index < rules.length; index += 1) {
    const rule = rules[index];
    groups.push({
      index,
      ruleId: null,
      ruleName: rule?.name ?? '',
      matched: null,
      itemIds: [],
      stepCount: rule?.stepKinds.length ?? 0,
      ranCount: 0,
      failedPosition: null,
      cut: null,
    });
  }
  const real = items.filter((item) => item.type === 'step' && !item.placeholder);
  const respond = real.filter((item) => item.stepKind === RESPOND_KIND && item.status === 'ran').at(-1);
  return {
    items,
    groups,
    final: respondSummary(respond, result),
    error: result.error,
    failed: result.state === 'failed' || result.error !== null,
    truncated: result.entriesTruncated,
    matchedCount: groups.filter((group) => group.matched === true).length,
    simulatedCount: real.filter((item) => item.status === 'simulated').length,
    stepCount: real.length,
    durationMs: real.reduce((sum, item) => sum + item.durationMs, 0),
  };
}

export function findStepItem(model: TraceModel, stepIndex: number, groupIndex: number): TraceItem | undefined {
  return model.items.find((item) => item.type === 'step' && item.stepIndex === stepIndex && item.group === groupIndex);
}
