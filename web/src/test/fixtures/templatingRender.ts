import type { Capture } from '../../api/types';
import type { PreviewDiagnostic, PreviewRequest, PreviewResult } from '../../features/templating/types';

interface RenderOutcome {
  text: string;
  diagnostics: PreviewDiagnostic[];
}

const TOKEN = /\{\{\s*([^}]*?)\s*\}\}/g;
const FILTERS: Readonly<Record<string, (value: string) => string>> = {
  upper: (value) => value.toUpperCase(),
  lower: (value) => value.toLowerCase(),
};
interface Position {
  line: number;
  column: number;
}

function positionOf(source: string, index: number): Position {
  const before = source.slice(0, index);
  const line = before.split('\n').length;
  const column = index - (before.lastIndexOf('\n') + 1) + 1;
  return { line, column };
}

function valuesOf(capture: Capture): Record<string, string> {
  return {
    'request.method': capture.method,
    'request.path': capture.path,
    'json("$.id")': `evt_${capture.seq}`,
    now: capture.received_at,
  };
}

function renderText(source: string, field: string, values: Record<string, string>): RenderOutcome {
  const diagnostics: PreviewDiagnostic[] = [];
  const text = source.replace(TOKEN, (match: string, expression: string, offset: number) => {
    const [path = '', ...filters] = expression.split('|').map((part) => part.trim());
    const unknown = filters.find((filter) => FILTERS[filter] === undefined);
    if (unknown !== undefined) {
      diagnostics.push({
        field,
        code: 'template_invalid',
        severity: 'error',
        ...positionOf(source, offset + match.indexOf(unknown)),
        length: unknown.length,
        message: `Unknown filter "${unknown}"`,
      });
      return '';
    }
    const value = values[path];
    if (value === undefined) {
      diagnostics.push({
        field,
        code: 'value_missing',
        severity: 'warning',
        ...positionOf(source, offset),
        length: match.length,
        message: `${path} has no value and renders empty`,
      });
      return '';
    }
    return filters.reduce((current, filter) => FILTERS[filter]?.(current) ?? current, value);
  });
  return { text, diagnostics };
}

export function renderFake(request: PreviewRequest, capture: Capture | null): PreviewResult {
  const values = capture === null ? {} : valuesOf(capture);
  const body = renderText(request.response.body.data, 'response.body', values);
  const headers = request.response.headers.map((header, index) => ({
    name: header.name,
    ...renderText(header.value, `response.headers.${index}.value`, values),
  }));
  const diagnostics = [...body.diagnostics, ...headers.flatMap((header) => header.diagnostics)];
  const failed = diagnostics.some((diagnostic) => diagnostic.severity === 'error');
  if (failed || capture === null) {
    return { capture_id: request.capture_id, rendered: null, diagnostics };
  }
  return {
    capture_id: capture.id,
    diagnostics,
    rendered: {
      status: request.response.status,
      content_type: request.response.content_type,
      headers: headers.map((header) => ({ name: header.name, value: header.text })),
      body: { encoding: 'utf8', data: body.text, truncated: false },
      body_size: new TextEncoder().encode(body.text).byteLength,
      duration_ms: 3,
    },
  };
}
