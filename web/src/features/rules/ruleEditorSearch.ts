export interface RuleEditorSearch {
  from?: string;
}

export function validateRuleEditorSearch(search: Record<string, unknown>): RuleEditorSearch {
  return typeof search.from === 'string' ? { from: search.from } : {};
}
