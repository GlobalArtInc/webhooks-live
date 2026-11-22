import type { EditorDiagnostic } from '../../editor/types';
import type { ScriptDiagnostics } from './types';

export const SCRIPT_SAMPLE_SOURCE = [
  '// Refuse unknown events, otherwise answer with the order id',
  'let body = json_parse(request.body);',
  'let digest = sha256(request.path);',
  'if body.event == "order.paid" {',
  '  #{ response: #{ status: 200, body: json_stringify(#{ ok: true, id: body.id }) } }',
  '} else {',
  '  #{ response: #{ status: 400, body: "unknown event" } }',
  '}',
].join('\n');

export const SCRIPT_RECORDED_SOURCE = [
  'let total = json_parse(request.body).items.len();',
  'let average = json_parse(request.body).sum / total;',
  '#{ vars: #{ average: average } }',
].join('\n');

export const SCRIPT_RUNTIME_DIAGNOSTIC: EditorDiagnostic = {
  line: 1,
  column: 13,
  length: 24,
  code: 'runtime',
  message: 'Property items is not found on this map',
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
        code: 'disabled_feature',
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
      code: 'syntax',
      message: 'Expecting an expression after the operator',
      severity: 'error',
    });
  }
  return found;
}
