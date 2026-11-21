import { isCaptureId } from '../../app/ids';

export type RuleEditorTab = 'build' | 'test' | 'runs';

export interface RuleEditorSearch {
  from?: string;
  tab?: RuleEditorTab;
  capture?: string;
  run?: string;
}

function isTab(value: unknown): value is RuleEditorTab {
  return value === 'build' || value === 'test' || value === 'runs';
}

function idOf(value: unknown): string | undefined {
  return typeof value === 'string' && isCaptureId(value) ? value : undefined;
}

export function validateRuleEditorSearch(search: Record<string, unknown>): RuleEditorSearch {
  const result: RuleEditorSearch = {};
  if (typeof search.from === 'string') {
    result.from = search.from;
  }
  if (isTab(search.tab)) {
    result.tab = search.tab;
  }
  const capture = idOf(search.capture);
  if (capture !== undefined) {
    result.capture = capture;
  }
  const run = idOf(search.run);
  if (run !== undefined) {
    result.run = run;
  }
  return result;
}
