import type { Completion, CompletionContext, CompletionResult, CompletionSource } from '@codemirror/autocomplete';
import type { CompletionEntry, CompletionSource as EntrySource, TemplateDelimiters } from './types';

const PREFIX_PATTERN = /[A-Za-z0-9_.$@:-]*/;
const VALID_FOR = /^[A-Za-z0-9_.$@:-]*$/;
const TYPING_IDENTIFIER = /[A-Za-z0-9_$.]$/;

export function insideTemplate(before: string, delimiters: TemplateDelimiters | null): boolean {
  if (delimiters === null) {
    return false;
  }
  const opened = before.lastIndexOf(delimiters.open);
  if (opened === -1) {
    return false;
  }
  return before.lastIndexOf(delimiters.close) < opened;
}

function toCompletion(entry: CompletionEntry): Completion {
  const completion: Completion = { label: entry.label, type: entry.kind ?? 'variable' };
  if (entry.insertText !== undefined) {
    completion.apply = entry.insertText;
  }
  if (entry.detail !== undefined) {
    completion.detail = entry.detail;
  }
  if (entry.description !== undefined) {
    const description = entry.description;
    completion.info = () => {
      const node = document.createElement('div');
      node.textContent = description;
      return node;
    };
  }
  return completion;
}

export interface CompletionSettings {
  source: EntrySource | null;
  delimiters: TemplateDelimiters | null;
}

export function completionBridge(readSettings: () => CompletionSettings): CompletionSource {
  return async (context: CompletionContext): Promise<CompletionResult | null> => {
    const { source, delimiters } = readSettings();
    if (source === null) {
      return null;
    }
    const word = context.matchBefore(PREFIX_PATTERN);
    const from = word === null ? context.pos : word.from;
    const inside = insideTemplate(context.state.doc.sliceString(0, from), delimiters);
    if (delimiters !== null && !inside && !context.explicit) {
      return null;
    }
    if (delimiters === null && !context.explicit && !TYPING_IDENTIFIER.test(word?.text ?? '')) {
      return null;
    }
    const entries = await source({ prefix: word?.text ?? '', insideTemplate: inside, explicit: context.explicit });
    if (context.aborted) {
      return null;
    }
    return { from, options: entries.map(toCompletion), validFor: VALID_FOR };
  };
}
