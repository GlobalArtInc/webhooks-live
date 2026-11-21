import type { InfiniteData } from '@tanstack/react-query';
import type { Capture } from '../../../api/types';
import type {
  RuleRun,
  RuleRunEntry,
  RuleRunFilter,
  RuleRunPage,
  RuleRunRuleEntry,
  RuleRunState,
  RuleRunStepEntry,
} from '../../../api/ruleRunTypes';

export type RuleRunPages = InfiniteData<RuleRunPage, string | null>;

export const CAPTURE_RUN_LIMIT = 10;
export const MAX_OUTPUT_CHARS = 4096;
const ACTIVE_STATES: readonly RuleRunState[] = ['sync', 'queued', 'running'];
const RESPOND_KIND = 'respond';
const OUTCOME_OK = 'ok';

export interface RuleGroup {
  ruleId: string;
  ruleName: string | null;
  matched: boolean;
  steps: RuleRunStepEntry[];
}

export interface BoundedText {
  text: string;
  truncated: boolean;
}

export interface ResponseFacts {
  status: number;
  ruleError: string | null;
  changedBy: string | null;
}

export function isActiveRun(run: RuleRun): boolean {
  return ACTIVE_STATES.includes(run.state);
}

export function isRuleEntry(entry: RuleRunEntry): entry is RuleRunRuleEntry {
  return 'matched' in entry;
}

export function groupEntries(entries: readonly RuleRunEntry[]): RuleGroup[] {
  const groups = new Map<string, RuleGroup>();
  const groupOf = (ruleId: string): RuleGroup => {
    const existing = groups.get(ruleId);
    if (existing !== undefined) {
      return existing;
    }
    const created: RuleGroup = { ruleId, ruleName: null, matched: true, steps: [] };
    groups.set(ruleId, created);
    return created;
  };
  for (const entry of entries) {
    if (isRuleEntry(entry)) {
      const group = groupOf(entry.rule_id);
      group.ruleName = entry.rule_name;
      group.matched = entry.matched;
    } else {
      groupOf(entry.rule_id).steps.push(entry);
    }
  }
  return [...groups.values()];
}

export function totalDuration(steps: readonly RuleRunStepEntry[]): number {
  return steps.reduce((sum, step) => sum + step.duration_ms, 0);
}

export function outputText(output: unknown): BoundedText | null {
  if (output === null || output === undefined) {
    return null;
  }
  const text = JSON.stringify(output, null, 2) ?? '';
  return text.length > MAX_OUTPUT_CHARS
    ? { text: text.slice(0, MAX_OUTPUT_CHARS), truncated: true }
    : { text, truncated: false };
}

function respondedOk(step: RuleRunStepEntry): boolean {
  return step.kind === RESPOND_KIND && step.outcome === OUTCOME_OK;
}

export function responseFacts(status: number, runs: readonly RuleRun[]): ResponseFacts {
  const original = runs.find((run) => run.trigger === 'capture') ?? null;
  if (original === null) {
    return { status, ruleError: null, changedBy: null };
  }
  const changers = groupEntries(original.entries).filter((group) => group.steps.some(respondedOk));
  return { status, ruleError: original.error, changedBy: changers.at(-1)?.ruleName ?? null };
}

export function withRuleRun(capture: Capture, run: RuleRun): Capture {
  return { ...capture, rule_runs: upsertRun(capture.rule_runs ?? [], run).slice(0, CAPTURE_RUN_LIMIT) };
}

export function upsertRun(runs: readonly RuleRun[], run: RuleRun): RuleRun[] {
  return runs.some((item) => item.id === run.id)
    ? runs.map((item) => (item.id === run.id ? run : item))
    : [run, ...runs];
}

export function matchesFilter(run: RuleRun, filter: RuleRunFilter): boolean {
  return filter.state === undefined || run.state === filter.state;
}

function replaceInPages(data: RuleRunPages, run: RuleRun): RuleRunPages {
  return {
    ...data,
    pages: data.pages.map((page) => ({
      ...page,
      items: page.items.map((item) => (item.id === run.id ? run : item)),
    })),
  };
}

function removeFromPages(data: RuleRunPages, runId: string): RuleRunPages {
  return {
    ...data,
    pages: data.pages.map((page) => ({ ...page, items: page.items.filter((item) => item.id !== runId) })),
  };
}

export function upsertRunInPages(data: RuleRunPages, run: RuleRun, filter: RuleRunFilter): RuleRunPages {
  if (!matchesFilter(run, filter)) {
    return removeFromPages(data, run.id);
  }
  const known = data.pages.some((page) => page.items.some((item) => item.id === run.id));
  if (known) {
    return replaceInPages(data, run);
  }
  const [first, ...rest] = data.pages;
  if (first === undefined) {
    return data;
  }
  return { ...data, pages: [{ ...first, items: [run, ...first.items] }, ...rest] };
}
