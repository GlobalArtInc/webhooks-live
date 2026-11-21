#!/usr/bin/env node
import path from 'node:path';
import { fileURLToPath } from 'node:url';
import { checkTree } from './checks.mjs';
import { PROJECT_ENV, TOKEN_ENV, pullTranslations } from './pull.mjs';

const ROOT = path.resolve(path.dirname(fileURLToPath(import.meta.url)), '../..');
const USAGE = 'Usage: node tools/i18n/i18n.mjs <check|pull>';

function runCheck() {
  const { errors, warnings } = checkTree(ROOT);
  warnings.forEach((warning) => console.warn(`warning: ${warning}`));
  errors.forEach((error) => console.error(`error: ${error}`));
  if (errors.length > 0) {
    process.exitCode = 1;
    return;
  }
  console.log('i18n: the English source and the pulled translations are consistent');
}

async function runPull() {
  const token = process.env[TOKEN_ENV];
  const project = process.env[PROJECT_ENV];
  if (!token || !project) {
    console.log(`${TOKEN_ENV} or ${PROJECT_ENV} is not set, skipping the pull`);
    return;
  }
  try {
    for (const message of await pullTranslations(ROOT, { token, project })) {
      console.log(message);
    }
  } catch (error) {
    console.error(error instanceof Error ? error.message : String(error));
    process.exitCode = 1;
  }
}

switch (process.argv[2]) {
  case 'check':
    runCheck();
    break;
  case 'pull':
    await runPull();
    break;
  default:
    console.error(USAGE);
    process.exitCode = 2;
}
