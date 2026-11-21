import fs from 'node:fs';
import os from 'node:os';
import path from 'node:path';

export function writeTree(files) {
  const root = fs.mkdtempSync(path.join(os.tmpdir(), 'webhooks-live-i18n-'));
  for (const [name, text] of Object.entries(files)) {
    const file = path.join(root, name);
    fs.mkdirSync(path.dirname(file), { recursive: true });
    fs.writeFileSync(file, text);
  }
  return root;
}

export function removeTree(root) {
  fs.rmSync(root, { recursive: true, force: true });
}
