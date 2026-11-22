import scriptMetaJson from '../../../test/fixtures/scriptMeta.json';
import { parseScriptMeta } from './parse';
import type { ScriptLanguageMeta } from './types';

export const SCRIPT_META_ANSWER = scriptMetaJson;

export const SCRIPT_META_FIXTURE: ScriptLanguageMeta = parseScriptMeta(SCRIPT_META_ANSWER);
