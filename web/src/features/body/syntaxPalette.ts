export type SyntaxRole = 'key' | 'attribute' | 'string' | 'number' | 'literal' | 'tag' | 'muted' | 'error';

export const SYNTAX_TOKENS: Readonly<Record<SyntaxRole, string>> = {
  key: 'color-accent-text',
  attribute: 'color-warning',
  string: 'color-success',
  number: 'color-info',
  literal: 'color-warning',
  tag: 'color-info',
  muted: 'color-text-muted',
  error: 'color-danger',
};

export function syntaxColour(role: SyntaxRole): string {
  return `var(--${SYNTAX_TOKENS[role]})`;
}
