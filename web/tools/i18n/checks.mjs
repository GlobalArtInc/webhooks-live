import fs from 'node:fs';
import path from 'node:path';

export const SOURCE_LANGUAGE = 'en';
export const SOURCE_DIR = path.join('src', 'i18n', SOURCE_LANGUAGE);
export const PULLED_DIR = path.join('src', 'i18n', 'pulled');
export const PLURAL_SUFFIX = /_(zero|one|two|few|many|other)$/;

const KEY_SHAPE = /^wl\.[a-z][a-z0-9]*\.[a-z][A-Za-z0-9]*(\.[a-z][A-Za-z0-9]*)*(_(zero|one|two|few|many|other))?$/;
const PLACEHOLDER = /\{\{\s*(\w+)\s*\}\}/g;
const CALL = /\bt\(\s*(['"`])(wl\.[^'"`\n]*)\1/g;
const ATTRIBUTE = /\bi18nKey\s*=\s*\{?\s*(['"`])(wl\.[^'"`\n]*)\1/g;

export function pluralBase(key) {
  return key.replace(PLURAL_SUFFIX, '');
}

export function checkTree(root) {
  const errors = [];
  const warnings = [];
  const source = readSource(root, errors);
  const references = readReferences(root);
  checkReferences(references, source, errors);
  checkUnused(references, source, warnings);
  checkPulled(root, source, errors, warnings);
  return { errors: [...new Set(errors)], warnings: [...new Set(warnings)] };
}

function relative(root, file) {
  return path.relative(root, file).split(path.sep).join('/');
}

function jsonFiles(dir) {
  if (!fs.existsSync(dir)) {
    return [];
  }
  return fs
    .readdirSync(dir)
    .filter((name) => name.endsWith('.json'))
    .sort()
    .map((name) => path.join(dir, name));
}

function sourceFiles(dir) {
  if (!fs.existsSync(dir)) {
    return [];
  }
  return fs
    .readdirSync(dir, { withFileTypes: true })
    .sort((left, right) => left.name.localeCompare(right.name))
    .flatMap((entry) => {
      const file = path.join(dir, entry.name);
      if (entry.isDirectory()) {
        return sourceFiles(file);
      }
      const isCode = /\.tsx?$/.test(entry.name) && !/\.test\.tsx?$/.test(entry.name);
      return isCode ? [file] : [];
    });
}

function readFlat(file, name, errors) {
  let parsed;
  try {
    parsed = JSON.parse(fs.readFileSync(file, 'utf8'));
  } catch {
    errors.push(`${name}: is not valid JSON`);
    return {};
  }
  if (typeof parsed !== 'object' || parsed === null || Array.isArray(parsed)) {
    errors.push(`${name}: is not a flat object of keys and strings`);
    return {};
  }
  return parsed;
}

export function readSource(root, errors) {
  const source = new Map();
  const dir = path.join(root, SOURCE_DIR);
  if (!fs.existsSync(dir)) {
    errors.push(`${relative(root, dir)} is missing, the English source is required`);
    return source;
  }
  for (const file of jsonFiles(dir)) {
    const name = relative(root, file);
    for (const [key, text] of Object.entries(readFlat(file, name, errors))) {
      if (!KEY_SHAPE.test(key)) {
        errors.push(`${name}: ${key} does not match wl.<area>.<element>[.<qualifier>]`);
      }
      if (typeof text !== 'string') {
        errors.push(`${name}: ${key} is not a string`);
        continue;
      }
      if (text.trim() === '') {
        errors.push(`${name}: ${key} is empty`);
      }
      if (source.has(key)) {
        errors.push(`${name}: ${key} is also defined in ${source.get(key).file}`);
      } else {
        source.set(key, { file: name, text });
      }
    }
  }
  checkPluralForms(source, errors);
  return source;
}

function checkPluralForms(source, errors) {
  const bases = new Map();
  for (const [key, { file }] of source) {
    if (PLURAL_SUFFIX.test(key) && !bases.has(pluralBase(key))) {
      bases.set(pluralBase(key), file);
    }
  }
  for (const [base, file] of bases) {
    if (!source.has(`${base}_other`)) {
      errors.push(`${file}: ${base} has no ${base}_other form`);
    }
  }
}

function readReferences(root) {
  const references = [];
  for (const file of sourceFiles(path.join(root, 'src'))) {
    const text = fs.readFileSync(file, 'utf8');
    for (const pattern of [CALL, ATTRIBUTE]) {
      for (const match of text.matchAll(pattern)) {
        references.push({ key: match[2], file: relative(root, file) });
      }
    }
  }
  return references;
}

function isKnown(source, key) {
  return source.has(key) || source.has(`${key}_other`);
}

function checkReferences(references, source, errors) {
  for (const { key, file } of references) {
    if (!isKnown(source, key)) {
      errors.push(`${file}: ${key} is used but missing from the English source`);
    }
  }
}

function checkUnused(references, source, warnings) {
  const used = new Set(references.map(({ key }) => key));
  for (const [key, { file }] of source) {
    const base = pluralBase(key);
    if (!used.has(key) && !used.has(base)) {
      warnings.push(`${file}: ${base} is never used in the code`);
    }
  }
}

function englishOf(source, key) {
  const direct = source.get(key);
  if (direct !== undefined) {
    return direct;
  }
  const base = pluralBase(key);
  return base === key ? undefined : source.get(`${base}_other`);
}

function placeholdersOf(text) {
  const names = [...text.matchAll(PLACEHOLDER)].map((match) => match[1]);
  return [...new Set(names)].sort().join(' ');
}

function checkPulled(root, source, errors, warnings) {
  for (const file of jsonFiles(path.join(root, PULLED_DIR))) {
    const name = relative(root, file);
    for (const [key, text] of Object.entries(readFlat(file, name, errors))) {
      if (typeof text !== 'string') {
        errors.push(`${name}: ${key} is not a string`);
        continue;
      }
      const english = englishOf(source, key);
      if (english === undefined) {
        warnings.push(`${name}: ${key} is not in the English source`);
        continue;
      }
      const found = placeholdersOf(text);
      const expected = placeholdersOf(english.text);
      if (found !== expected) {
        errors.push(`${name}: ${key} has placeholders [${found}], English has [${expected}]`);
      }
    }
  }
}
