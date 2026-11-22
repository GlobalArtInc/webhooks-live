import { createHash } from 'node:crypto';
import { readFileSync } from 'node:fs';
import { fileURLToPath } from 'node:url';
import type { Page } from '@playwright/test';

const SHELL_SOURCE = fileURLToPath(new URL('../../../crates/whl-web/src/shell.rs', import.meta.url));
const POLICY_TEMPLATE = /"(default-src 'none'; script-src \{script_src\};[^"]*)"/;
const SCRIPT_BLOCK = /<script>([\s\S]*?)<\/script>/g;
const VIOLATION_KEY = '__cspViolations';

export interface CspViolation {
  directive: string;
  blocked: string;
  sample: string;
  source: string;
  line: number;
  column: number;
}

export interface CspReport {
  violations: CspViolation[];
  consoleErrors: string[];
}

function policyTemplate(): string {
  const match = POLICY_TEMPLATE.exec(readFileSync(SHELL_SOURCE, 'utf8'));
  if (match?.[1] === undefined) {
    throw new Error(`the policy template was not found in ${SHELL_SOURCE}`);
  }
  return match[1];
}

export function productionPolicy(html: string): string {
  const hashes = [...html.matchAll(SCRIPT_BLOCK)].map(
    (block) =>
      `'sha256-${createHash('sha256')
        .update(block[1] ?? '')
        .digest('base64')}'`,
  );
  return policyTemplate().replace('{script_src}', ["'self'", ...hashes].join(' '));
}

export async function enforceProductionCsp(page: Page): Promise<CspReport> {
  const report: CspReport = { violations: [], consoleErrors: [] };
  await page.addInitScript((key) => {
    const store: unknown[] = [];
    Reflect.set(window, key, store);
    document.addEventListener('securitypolicyviolation', (event) => {
      store.push({
        directive: event.violatedDirective,
        blocked: event.blockedURI,
        sample: event.sample,
        source: event.sourceFile,
        line: event.lineNumber,
        column: event.columnNumber,
      });
    });
  }, VIOLATION_KEY);
  page.on('console', (message) => {
    if (message.type() === 'error' && /Content Security Policy/i.test(message.text())) {
      report.consoleErrors.push(message.text());
    }
  });
  await page.route(
    (url) => url.pathname === '/' || !url.pathname.includes('.'),
    async (route) => {
      if (route.request().resourceType() !== 'document') {
        await route.fallback();
        return;
      }
      const response = await route.fetch();
      const html = await response.text();
      await route.fulfill({
        response,
        body: html,
        headers: { ...response.headers(), 'content-security-policy': productionPolicy(html) },
      });
    },
  );
  return report;
}

export async function collectViolations(page: Page, report: CspReport): Promise<CspReport> {
  const stored = await page.evaluate((key) => Reflect.get(window, key) as CspViolation[], VIOLATION_KEY);
  return { violations: stored, consoleErrors: report.consoleErrors };
}
