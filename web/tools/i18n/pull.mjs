import fs from 'node:fs';
import path from 'node:path';
import { PULLED_DIR, SOURCE_LANGUAGE, pluralBase, readSource } from './checks.mjs';

export const OWNLATE_URL = 'https://api.ownlate.com';
export const TOKEN_ENV = 'WEBHOOKS_LIVE_OWNLATE_TOKEN';
export const PROJECT_ENV = 'WEBHOOKS_LIVE_OWNLATE_PROJECT';

const PLURAL_FORMS = ['zero', 'one', 'two', 'few', 'many', 'other'];
const LANGUAGE_CODE = /^[A-Za-z][A-Za-z0-9_-]*$/;

export async function pullTranslations(root, { token, project, fetchImpl = fetch }) {
  const errors = [];
  const source = readSource(root, errors);
  if (errors.length > 0) {
    throw new Error(errors.join('\n'));
  }

  const url = `${OWNLATE_URL}/public/v1/segments/translations-map?projectId=${encodeURIComponent(project)}`;
  const response = await fetchImpl(url, {
    headers: { Authorization: `Bearer ${token}`, Accept: 'application/json' },
  });
  const body = await response.text();
  if (response.status !== 200) {
    throw new Error(`${url} answered ${response.status}: ${body.slice(0, 300)}`);
  }

  const messages = [];
  for (const [language, texts] of Object.entries(messagesByLanguage(JSON.parse(body)))) {
    if (!isTarget(language)) {
      continue;
    }
    messages.push(writeLanguage(root, language, translatedEntries(source, language, texts)));
  }
  return messages;
}

function isTarget(language) {
  return LANGUAGE_CODE.test(language) && language !== SOURCE_LANGUAGE && !language.startsWith(`${SOURCE_LANGUAGE}_`);
}

function messagesByLanguage(payload) {
  const byLanguage = {};
  for (const files of Object.values(payload)) {
    for (const [language, messages] of Object.entries(files)) {
      byLanguage[language] = flatten(messages, '', byLanguage[language] ?? {});
    }
  }
  return byLanguage;
}

function flatten(node, prefix, into) {
  for (const [key, value] of Object.entries(node)) {
    const name = prefix ? `${prefix}.${key}` : key;
    if (value && typeof value === 'object') {
      flatten(value, name, into);
    } else if (typeof value === 'string' && value !== '') {
      into[name] = value;
    }
  }
  return into;
}

function pluralForms(language) {
  const categories = new Intl.PluralRules(language).resolvedOptions().pluralCategories;
  return PLURAL_FORMS.filter((form) => categories.includes(form));
}

function translatedEntries(source, language, texts) {
  const forms = pluralForms(language);
  const entries = {};
  const seenBases = new Set();
  for (const key of source.keys()) {
    const base = pluralBase(key);
    if (base === key) {
      if (Object.hasOwn(texts, key)) {
        entries[key] = texts[key];
      }
    } else if (!seenBases.has(base)) {
      seenBases.add(base);
      const names = forms.map((form) => `${base}_${form}`);
      if (names.every((name) => Object.hasOwn(texts, name))) {
        for (const name of names) {
          entries[name] = texts[name];
        }
      }
    }
  }
  return entries;
}

function writeLanguage(root, language, entries) {
  const file = path.join(root, PULLED_DIR, `${language}.json`);
  const name = path.relative(root, file).split(path.sep).join('/');
  if (Object.keys(entries).length === 0) {
    if (fs.existsSync(file)) {
      fs.unlinkSync(file);
      return `removed ${name} (no translations)`;
    }
    return `skipped ${name} (no translations)`;
  }
  fs.mkdirSync(path.dirname(file), { recursive: true });
  fs.writeFileSync(file, `${JSON.stringify(entries, null, 2)}\n`);
  return `wrote ${name} (${Object.keys(entries).length} entries)`;
}
