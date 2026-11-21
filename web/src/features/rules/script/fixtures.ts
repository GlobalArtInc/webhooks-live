import type { EditorDiagnostic } from '../../editor/types';
import type { ScriptDiagnostics, ScriptLanguageMeta } from './types';

export const SCRIPT_META_FIXTURE: ScriptLanguageMeta = {
  name: 'rhai',
  keywords: ['let', 'const', 'if', 'else', 'switch', 'while', 'loop', 'for', 'in', 'fn', 'return', 'break', 'continue'],
  functions: [
    { name: 'json_parse', signature: 'json_parse(text: string) -> any', doc: 'Parses JSON text into maps and arrays.' },
    {
      name: 'json_stringify',
      signature: 'json_stringify(value: any) -> string',
      doc: 'Writes a value as compact JSON text.',
    },
    {
      name: 'base64_encode',
      signature: 'base64_encode(text: string) -> string',
      doc: 'Encodes text as standard base64.',
    },
    {
      name: 'sha256',
      signature: 'sha256(text: string) -> string',
      doc: 'Returns the SHA-256 digest of the text as lowercase hex.',
    },
    {
      name: 'hmac_sha256',
      signature: 'hmac_sha256(key: string, text: string) -> string',
      doc: 'Returns the HMAC-SHA256 of the text as lowercase hex.',
    },
    {
      name: 'regex_match',
      signature: 'regex_match(text: string, pattern: string) -> bool',
      doc: 'True when the pattern matches somewhere in the text.',
    },
    { name: 'uuid', signature: 'uuid() -> string', doc: 'Returns a new random UUID.' },
    { name: 'now', signature: 'now() -> string', doc: 'Returns the capture time as an RFC 3339 timestamp.' },
    { name: 'print', signature: 'print(value: any)', doc: 'Writes a line to the run log, 4 KiB in total.' },
  ],
  variables: [
    { name: 'request', type: 'map', doc: 'The captured request, read-only.' },
    { name: 'request.method', type: 'string', doc: 'The HTTP method, upper case.' },
    { name: 'request.path', type: 'string', doc: 'The path after the endpoint id.' },
    { name: 'request.json', type: 'any', doc: 'The body parsed as JSON, or an empty map.' },
    { name: 'request.header', type: 'map', doc: 'Header values by lower-case name.' },
    { name: 'vars', type: 'map', doc: 'Run variables. Changes leave the script only through the returned map.' },
  ],
  limits: {
    max_source_bytes: 16384,
    before_response: { max_operations: 100000, max_time_ms: 100 },
    after_response: { max_operations: 1000000, max_time_ms: 1000 },
  },
  reference_url: '/docs/scripting',
};

export const SCRIPT_SAMPLE_SOURCE = [
  '// Refuse unknown events, otherwise answer with the order id',
  'let event = request.json.event;',
  'let digest = sha256(request.path);',
  'if event == "order.paid" {',
  '  #{ response: #{ status: 200, body: json_stringify(#{ ok: true, id: request.json.id }) } }',
  '} else {',
  '  #{ response: #{ status: 400, body: "unknown event" } }',
  '}',
].join('\n');

export const SCRIPT_RECORDED_SOURCE = [
  'let total = request.json.items.len();',
  'let average = request.json.sum / total;',
  '#{ vars: #{ average: average } }',
].join('\n');

export const SCRIPT_RUNTIME_DIAGNOSTIC: EditorDiagnostic = {
  line: 1,
  column: 13,
  length: 18,
  code: 'script_runtime',
  message: 'Property request.json.items is not found on this map',
  severity: 'error',
};

const TRAILING_OPERATOR = /(?:\+|-|\*|\/|&&|\|\||==)\s*$/;
const DISABLED_SYMBOL = /\b(eval|import)\b/;
const DEBUG_CALL = /\bdebug\(/;

export function fixtureValidate(source: string): ScriptDiagnostics {
  const found: ScriptDiagnostics = [];
  const lines = source.split('\n');
  lines.forEach((text, index) => {
    const line = index + 1;
    const disabled = DISABLED_SYMBOL.exec(text);
    if (disabled !== null) {
      found.push({
        line,
        column: disabled.index + 1,
        length: disabled[0].length,
        code: 'script_symbol_disabled',
        message: `${disabled[0]} is disabled in the sandbox`,
        severity: 'error',
      });
    }
    const debug = DEBUG_CALL.exec(text);
    if (debug !== null) {
      found.push({
        line,
        column: debug.index + 1,
        length: 5,
        code: 'script_log_budget',
        message: 'debug output counts against the 4 KiB run log',
        severity: 'warning',
      });
    }
  });
  const lastIndex = lines.reduce((last, text, index) => (text.trim() === '' ? last : index), -1);
  const lastText = lastIndex === -1 ? '' : (lines[lastIndex] ?? '');
  if (TRAILING_OPERATOR.test(lastText)) {
    found.push({
      line: lastIndex + 1,
      column: lastText.trimEnd().length,
      length: 1,
      code: 'script_invalid',
      message: 'Expecting an expression after the operator',
      severity: 'error',
    });
  }
  return found;
}
