export type EditorLanguage = 'text' | 'template' | 'json' | 'html' | 'xml' | 'script';

export type DiagnosticSeverity = 'error' | 'warning';

export interface EditorDiagnostic {
  line: number;
  column: number;
  length: number;
  code: string;
  message: string;
  severity?: DiagnosticSeverity;
}

export interface TemplateDelimiters {
  open: string;
  close: string;
}

export type CompletionKind = 'variable' | 'function' | 'keyword' | 'helper';

export interface CompletionEntry {
  label: string;
  insertText?: string;
  detail?: string;
  description?: string;
  kind?: CompletionKind;
}

export interface CompletionQuery {
  prefix: string;
  insideTemplate: boolean;
  explicit: boolean;
}

export type CompletionResult = readonly CompletionEntry[] | Promise<readonly CompletionEntry[]>;

export type CompletionSource = (query: CompletionQuery) => CompletionResult;

export interface TextRange {
  from: number;
  to: number;
}

export interface EngineConfig {
  value: string;
  language: EditorLanguage;
  label: string;
  readOnly: boolean;
  singleLine: boolean;
  invalid: boolean;
  delimiters: TemplateDelimiters | null;
  scriptKeywords: readonly string[] | null;
  diagnostics: readonly EditorDiagnostic[];
  completions: CompletionSource | null;
  onChange: ((value: string) => void) | null;
  onSubmit: (() => void) | null;
  onTabEscapeChange: ((active: boolean) => void) | null;
}

export interface EngineHandle {
  update: (config: EngineConfig) => void;
  focus: () => void;
  insertAtCursor: (text: string) => void;
  goTo: (line: number, column: number) => void;
  destroy: () => void;
}

export interface CodeEditorHandle {
  focus: () => void;
  insertAtCursor: (text: string) => void;
  goTo: (line: number, column: number) => void;
}
