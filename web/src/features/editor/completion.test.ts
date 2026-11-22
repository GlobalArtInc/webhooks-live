import { CompletionContext } from '@codemirror/autocomplete';
import { EditorState } from '@codemirror/state';
import { describe, expect, it } from 'vitest';
import { completionBridge } from './completion';
import type { CompletionEntry, TemplateDelimiters } from './types';

const ENTRIES: CompletionEntry[] = [{ label: 'request' }, { label: 'respond' }];
const TEMPLATE: TemplateDelimiters = { open: '{{', close: '}}' };

async function labelsAt(
  doc: string,
  explicit: boolean,
  delimiters: TemplateDelimiters | null = null,
): Promise<string[] | null> {
  const bridge = completionBridge(() => ({ source: () => ENTRIES, delimiters }));
  const state = EditorState.create({ doc });
  const result = await bridge(new CompletionContext(state, doc.length, explicit));
  return result === null ? null : result.options.map((option) => option.label);
}

describe('completion bridge in a plain editor', () => {
  it('opens while an identifier is being typed and after a dot', async () => {
    expect(await labelsAt('let a = re', false)).toEqual(['request', 'respond']);
    expect(await labelsAt('request.', false)).toEqual(['request', 'respond']);
  });

  it('stays closed after operators and punctuation unless it is invoked explicitly', async () => {
    for (const doc of ['let a = ', 'let a =', 'let a = ;', 'foo(', 'a, ', 'a + ']) {
      expect(await labelsAt(doc, false)).toBeNull();
    }
    expect(await labelsAt('let a = ', true)).toEqual(['request', 'respond']);
  });
});

describe('completion bridge in a template editor', () => {
  it('keeps opening inside a template and stays closed outside it unless explicit', async () => {
    expect(await labelsAt('{{ ', false, TEMPLATE)).toEqual(['request', 'respond']);
    expect(await labelsAt('text ', false, TEMPLATE)).toBeNull();
    expect(await labelsAt('text ', true, TEMPLATE)).toEqual(['request', 'respond']);
  });
});
