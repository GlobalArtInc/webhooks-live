import type { EditorDiagnostic } from '../../editor/types';

export type ScriptPhase = 'before' | 'after';

export interface ScriptFunctionDoc {
  name: string;
  signature: string;
  doc: string;
}

export interface ScriptVariableDoc {
  name: string;
  type: string;
  doc: string;
}

export interface ScriptPhaseLimits {
  max_operations: number;
  max_time_ms: number;
}

export interface ScriptLimits {
  max_source_bytes: number;
  before_response: ScriptPhaseLimits;
  after_response: ScriptPhaseLimits;
}

export interface ScriptLanguageMeta {
  name: string;
  keywords: string[];
  functions: ScriptFunctionDoc[];
  variables: ScriptVariableDoc[];
  limits: ScriptLimits;
  reference_url: string | null;
}

export type ScriptDiagnostics = EditorDiagnostic[];

export type ScriptLanguageState = 'loading' | 'ready' | 'failed';

export type ScriptValidationState = 'idle' | 'pending' | 'ready' | 'failed';

export interface ScriptTransport {
  id: string;
  loadMeta: (signal: AbortSignal) => Promise<ScriptLanguageMeta>;
  validate: (source: string, signal: AbortSignal) => Promise<ScriptDiagnostics>;
}

export type HoverKind = 'function' | 'variable';

export interface HoverEntry {
  kind: HoverKind;
  title: string;
  detail: string;
  doc: string;
}

export interface HoverTarget {
  from: number;
  to: number;
  entry: HoverEntry;
}

export interface EnhancementConfig {
  meta: ScriptLanguageMeta | null;
  fullHeight: boolean;
}

export interface Enhancements {
  update: (config: EnhancementConfig) => void;
  destroy: () => void;
}
