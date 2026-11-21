import { autocompletion, startCompletion } from '@codemirror/autocomplete';
import { defaultKeymap, history, historyKeymap, indentWithTab, temporarilySetTabFocusMode } from '@codemirror/commands';
import { bracketMatching, syntaxHighlighting } from '@codemirror/language';
import { lintGutter, setDiagnostics, type Diagnostic } from '@codemirror/lint';
import { Annotation, Compartment, EditorSelection, EditorState, Prec, type Extension } from '@codemirror/state';
import { drawSelection, EditorView, keymap, lineNumbers, type KeyBinding, type ViewUpdate } from '@codemirror/view';
import { editorRoot, highlightStyle, languageExtension, viewerTheme } from '../body/editorTheme';
import { completionBridge } from './completion';
import { diagnosticRange, positionToOffset } from './positions';
import { scriptLanguage } from './scriptLanguage';
import { templateMarks } from './templateMarks';
import type { EditorDiagnostic, EditorLanguage, EngineConfig, EngineHandle, TemplateDelimiters } from './types';

const External = Annotation.define<boolean>();

const TAB_ESCAPE_MS = 2000;
const MODIFIER_KEYS: readonly string[] = ['Shift', 'Control', 'Alt', 'Meta', 'Escape'];
const LINE_BREAKS = /\r?\n/g;

const editorTheme = EditorView.theme({
  '.cm-cursor': {
    borderLeftColor: 'var(--color-text)',
  },
  '.cm-wl-template': {
    color: 'var(--color-accent-text)',
    backgroundColor: 'var(--color-accent-soft)',
    borderRadius: 'var(--radius-sm)',
  },
  '.cm-lintRange': {
    backgroundImage: 'none',
    textDecorationLine: 'underline',
    textDecorationStyle: 'wavy',
    textUnderlineOffset: '3px',
  },
  '.cm-lintRange-error': {
    backgroundImage: 'none',
    textDecorationColor: 'var(--color-danger)',
  },
  '.cm-lintRange-warning': {
    backgroundImage: 'none',
    textDecorationColor: 'var(--color-warning)',
  },
  '.cm-lintRange-active': {
    backgroundColor: 'var(--color-accent-soft)',
  },
  '.cm-gutter-lint': {
    width: 'var(--space-4)',
  },
  '.cm-lint-marker': {
    width: 'auto',
    height: 'auto',
    font: 'var(--mono-data)',
    fontWeight: '600',
    textAlign: 'center',
  },
  '.cm-lint-marker-error': {
    content: 'none',
    color: 'var(--color-danger)',
  },
  '.cm-lint-marker-error::before': {
    content: '"!"',
  },
  '.cm-lint-marker-warning': {
    content: 'none',
    color: 'var(--color-warning)',
  },
  '.cm-lint-marker-warning::before': {
    content: '"~"',
  },
  '.cm-tooltip': {
    font: 'var(--text-data)',
    color: 'var(--color-text)',
    backgroundColor: 'var(--color-surface-raised)',
    border: '1px solid var(--color-border-strong)',
    borderRadius: 'var(--radius-md)',
    boxShadow: 'var(--elevation-1)',
  },
  '.cm-diagnostic-error': {
    borderLeft: '5px solid var(--color-danger)',
  },
  '.cm-diagnostic-warning': {
    borderLeft: '5px solid var(--color-warning)',
  },
  '.cm-tooltip-autocomplete > ul > li': {
    padding: 'var(--space-0) var(--space-2)',
  },
  '.cm-tooltip-autocomplete > ul > li[aria-selected]': {
    backgroundColor: 'var(--color-accent-soft)',
    color: 'var(--color-text)',
  },
  '.cm-completionDetail': {
    color: 'var(--color-text-muted)',
    fontStyle: 'normal',
  },
  '.cm-completionInfo': {
    font: 'var(--text-meta)',
    color: 'var(--color-text-muted)',
  },
});

const multiLineTheme = EditorView.theme({
  '&': {
    minHeight: 'calc(var(--editor-rows, 6) * 1lh + var(--space-4))',
  },
});

const singleLineTheme = EditorView.theme({
  '&': {
    minHeight: 'var(--space-6)',
    maxHeight: 'none',
  },
  '.cm-scroller': {
    overflowY: 'hidden',
  },
  '.cm-content': {
    padding: 'var(--space-1) 0',
  },
});

function languageFor(config: EngineConfig): Extension {
  const language: EditorLanguage = config.language;
  switch (language) {
    case 'json':
    case 'html':
    case 'xml':
      return languageExtension(language);
    case 'script':
      return scriptLanguage(config.scriptKeywords);
    case 'text':
    case 'template':
      return [];
  }
}

function toDiagnostics(text: string, diagnostics: readonly EditorDiagnostic[]): Diagnostic[] {
  return diagnostics.map((diagnostic) => {
    const range = diagnosticRange(text, diagnostic);
    return {
      from: range.from,
      to: range.to,
      severity: diagnostic.severity ?? 'error',
      message: diagnostic.message,
      source: diagnostic.code,
    };
  });
}

function diagnosticsKey(diagnostics: readonly EditorDiagnostic[]): string {
  return JSON.stringify(diagnostics);
}

function keywordsKey(keywords: readonly string[] | null): string {
  return keywords === null ? '' : keywords.join('\u0000');
}

function delimitersKey(delimiters: TemplateDelimiters | null): string {
  return delimiters === null ? '' : `${delimiters.open}\u0000${delimiters.close}`;
}

export function createEngine(parent: HTMLElement, initial: EngineConfig): EngineHandle {
  let current = initial;
  let destroyed = false;
  let tabEscapeTimer: ReturnType<typeof setTimeout> | null = null;

  const languageSlot = new Compartment();
  const templateSlot = new Compartment();
  const modeSlot = new Compartment();
  const attributeSlot = new Compartment();

  const root = editorRoot(parent);

  function setTabEscape(active: boolean): void {
    if (tabEscapeTimer !== null) {
      clearTimeout(tabEscapeTimer);
      tabEscapeTimer = null;
    }
    if (active) {
      tabEscapeTimer = setTimeout(() => {
        tabEscapeTimer = null;
        current.onTabEscapeChange?.(false);
      }, TAB_ESCAPE_MS);
    }
    current.onTabEscapeChange?.(active);
  }

  function modeExtensions(config: EngineConfig): Extension {
    const submit: KeyBinding = {
      key: 'Mod-Enter',
      run: () => {
        const handler = current.onSubmit;
        if (handler === null) {
          return false;
        }
        handler();
        return true;
      },
    };
    const base = defaultKeymap.filter((binding) => binding.key !== 'Mod-Enter' && binding.key !== 'Escape');
    if (config.readOnly) {
      return [
        EditorState.readOnly.of(true),
        EditorView.editable.of(false),
        lineNumbers(),
        multiLineTheme,
        Prec.high(keymap.of([submit])),
        keymap.of(base),
      ];
    }
    const escape: KeyBinding = {
      key: 'Escape',
      run: (view) => {
        temporarilySetTabFocusMode(view);
        setTabEscape(true);
        return true;
      },
    };
    const common: Extension[] = [
      history(),
      Prec.high(keymap.of([submit])),
      keymap.of([escape, ...base, ...historyKeymap]),
    ];
    if (config.singleLine) {
      return [
        ...common,
        singleLineTheme,
        Prec.high(keymap.of([{ key: 'Enter', run: () => true }])),
        EditorView.clipboardInputFilter.of((text) => text.replace(LINE_BREAKS, ' ')),
        EditorState.transactionFilter.of((transaction) =>
          transaction.docChanged && transaction.newDoc.lines > 1 && transaction.annotation(External) !== true
            ? []
            : transaction,
        ),
      ];
    }
    return [...common, multiLineTheme, keymap.of([indentWithTab]), lineNumbers(), lintGutter(), bracketMatching()];
  }

  function attributeExtension(config: EngineConfig): Extension {
    const attributes: Record<string, string> = {
      'aria-label': config.label,
      'aria-invalid': config.invalid ? 'true' : 'false',
      spellcheck: 'false',
      autocapitalize: 'off',
      autocorrect: 'off',
    };
    if (config.readOnly) {
      attributes.tabindex = '0';
      attributes['aria-readonly'] = 'true';
    }
    if (config.singleLine) {
      attributes['aria-multiline'] = 'false';
    }
    return EditorView.contentAttributes.of(attributes);
  }

  function marksFor(config: EngineConfig): Extension {
    return config.delimiters === null ? [] : templateMarks(config.delimiters);
  }

  function onUpdate(update: ViewUpdate): void {
    if (!update.docChanged) {
      return;
    }
    const external = update.transactions.some((transaction) => transaction.annotation(External) === true);
    if (external) {
      return;
    }
    current.onChange?.(update.state.doc.toString());
    const delimiters = current.delimiters;
    const typed = update.transactions.some((transaction) => transaction.isUserEvent('input.type'));
    if (typed && delimiters !== null && current.completions !== null) {
      const head = update.state.selection.main.head;
      const before = update.state.doc.sliceString(Math.max(0, head - delimiters.open.length), head);
      if (before === delimiters.open) {
        const view = update.view;
        setTimeout(() => {
          if (!destroyed) {
            startCompletion(view);
          }
        }, 0);
      }
    }
  }

  const view = new EditorView({
    parent: root,
    root,
    state: EditorState.create({
      doc: initial.value,
      extensions: [
        drawSelection(),
        syntaxHighlighting(highlightStyle),
        viewerTheme,
        editorTheme,
        autocompletion({
          override: [completionBridge(() => ({ source: current.completions, delimiters: current.delimiters }))],
          icons: false,
        }),
        EditorView.updateListener.of(onUpdate),
        EditorView.domEventHandlers({
          keydown: (event) => {
            if (!MODIFIER_KEYS.includes(event.key)) {
              setTabEscape(false);
            }
            return false;
          },
          blur: () => {
            setTabEscape(false);
            return false;
          },
        }),
        languageSlot.of(languageFor(initial)),
        templateSlot.of(marksFor(initial)),
        modeSlot.of(modeExtensions(initial)),
        attributeSlot.of(attributeExtension(initial)),
      ],
    }),
  });

  let shownDiagnostics = diagnosticsKey(initial.diagnostics);
  if (initial.diagnostics.length > 0) {
    view.dispatch(setDiagnostics(view.state, toDiagnostics(initial.value, initial.diagnostics)));
  }

  return {
    update: (next: EngineConfig): void => {
      const previous = current;
      current = next;
      const effects = [];
      if (
        next.language !== previous.language ||
        keywordsKey(next.scriptKeywords) !== keywordsKey(previous.scriptKeywords)
      ) {
        effects.push(languageSlot.reconfigure(languageFor(next)));
      }
      if (delimitersKey(next.delimiters) !== delimitersKey(previous.delimiters)) {
        effects.push(templateSlot.reconfigure(marksFor(next)));
      }
      if (next.readOnly !== previous.readOnly || next.singleLine !== previous.singleLine) {
        effects.push(modeSlot.reconfigure(modeExtensions(next)));
      }
      if (
        next.label !== previous.label ||
        next.invalid !== previous.invalid ||
        next.readOnly !== previous.readOnly ||
        next.singleLine !== previous.singleLine
      ) {
        effects.push(attributeSlot.reconfigure(attributeExtension(next)));
      }
      const replace = next.value !== view.state.doc.toString();
      if (replace || effects.length > 0) {
        view.dispatch({
          changes: replace ? { from: 0, to: view.state.doc.length, insert: next.value } : undefined,
          annotations: External.of(true),
          effects,
        });
      }
      const key = diagnosticsKey(next.diagnostics);
      if (key !== shownDiagnostics || (replace && next.diagnostics.length > 0)) {
        shownDiagnostics = key;
        view.dispatch(setDiagnostics(view.state, toDiagnostics(view.state.doc.toString(), next.diagnostics)));
      }
    },
    focus: (): void => {
      view.focus();
    },
    insertAtCursor: (text: string): void => {
      if (view.state.readOnly) {
        return;
      }
      view.dispatch(view.state.update(view.state.replaceSelection(text), { scrollIntoView: true, userEvent: 'input' }));
      view.focus();
    },
    goTo: (line: number, column: number): void => {
      const offset = positionToOffset(view.state.doc.toString(), line, column);
      view.dispatch({ selection: EditorSelection.cursor(offset), scrollIntoView: true });
      view.focus();
    },
    destroy: (): void => {
      destroyed = true;
      if (tabEscapeTimer !== null) {
        clearTimeout(tabEscapeTimer);
      }
      view.destroy();
    },
  };
}
