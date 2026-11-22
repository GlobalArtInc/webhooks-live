import type { Page, Route } from '@playwright/test';
import { fixtureValidate } from '../../src/features/rules/script/fixtures';
import { SCRIPT_META_ANSWER } from './scriptMeta';

export const SLOW_MARKER = 'slowmark';

export const STALE_MESSAGE = 'Stale answer from an old request';

const SLOW_DELAY_MS = 1200;

export interface ScriptApiState {
  metaFails: boolean;
  metaCalls: number;
  sources: string[];
  released: string[];
}

interface ValidateBody {
  source: string;
}

function readSource(route: Route): string {
  const body = route.request().postDataJSON() as ValidateBody;
  return body.source;
}

function staleAnswer() {
  return {
    diagnostics: [{ line: 1, column: 1, length: 1, code: 'script_stale', message: STALE_MESSAGE, severity: 'error' }],
  };
}

export async function mockScriptApi(page: Page): Promise<ScriptApiState> {
  const state: ScriptApiState = { metaFails: false, metaCalls: 0, sources: [], released: [] };

  await page.route('**/api/v1/script/meta', async (route) => {
    state.metaCalls += 1;
    if (state.metaFails) {
      await route.fulfill({
        status: 503,
        contentType: 'application/json',
        body: JSON.stringify({ error: { code: 'feature_disabled', message: 'Scripts are off' } }),
      });
      return;
    }
    await route.fulfill({ status: 200, contentType: 'application/json', body: JSON.stringify(SCRIPT_META_ANSWER) });
  });

  await page.route('**/api/v1/script/validate', async (route) => {
    const source = readSource(route);
    state.sources.push(source);
    if (source.includes(SLOW_MARKER)) {
      await new Promise((resolve) => setTimeout(resolve, SLOW_DELAY_MS));
      await route
        .fulfill({ status: 200, contentType: 'application/json', body: JSON.stringify(staleAnswer()) })
        .catch(() => undefined);
      state.released.push(source);
      return;
    }
    await route.fulfill({
      status: 200,
      contentType: 'application/json',
      body: JSON.stringify({ diagnostics: fixtureValidate(source) }),
    });
  });

  return state;
}
