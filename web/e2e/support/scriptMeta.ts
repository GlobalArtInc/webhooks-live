import { readFileSync } from 'node:fs';
import { fileURLToPath } from 'node:url';

const META_PATH = fileURLToPath(new URL('../../src/test/fixtures/scriptMeta.json', import.meta.url));

export const SCRIPT_META_ANSWER: unknown = JSON.parse(readFileSync(META_PATH, 'utf8'));
