import type { Body, BodyInput, NameValue } from '../../api/types';
import type { TemplateDelimiters } from '../editor/types';

export type PreviewSeverity = 'error' | 'warning';

export interface CatalogueVariable {
  path: string;
  insert_text: string;
  group: string;
  type: string | null;
  sample: string | null;
  description: string | null;
}

export interface VariableCatalogue {
  delimiters: TemplateDelimiters;
  items: CatalogueVariable[];
}

export interface PreviewResponseInput {
  status: number;
  content_type: string;
  headers: NameValue[];
  body: BodyInput;
}

export interface PreviewRequest {
  capture_id: string | null;
  response: PreviewResponseInput;
}

export interface PreviewDiagnostic {
  field: string;
  code: string;
  severity: PreviewSeverity;
  line: number;
  column: number;
  length: number;
  message: string;
}

export interface RenderedResponse {
  status: number;
  content_type: string;
  headers: NameValue[];
  body: Body;
  body_size: number;
  duration_ms: number;
}

export interface PreviewResult {
  capture_id: string | null;
  rendered: RenderedResponse | null;
  diagnostics: PreviewDiagnostic[];
}

export type PreviewPhase = 'idle' | 'waiting' | 'rendering' | 'done' | 'failed';

export interface PreviewState {
  phase: PreviewPhase;
  result: PreviewResult | null;
  resultKey: string | null;
  lastRendered: RenderedResponse | null;
  error: unknown;
}

export type RenderPreview = (request: PreviewRequest, signal: AbortSignal) => Promise<PreviewResult>;
