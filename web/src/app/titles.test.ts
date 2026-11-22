import { afterEach, describe, expect, it } from 'vitest';
import { createI18nInstance } from '../i18n/setup';
import { setBaseTitle, setUnreadTitle } from './pageTitle';
import { pageTitleOf, shortTitleId } from './titles';

const i18n = createI18nInstance();
const ENDPOINT_ID = 'x7gabcdefghjkmnpqrstvwq3';

function titleOf(pathname: string): string {
  return pageTitleOf(pathname, i18n.t);
}

describe('pageTitleOf', () => {
  it('names the endpoint by the ends of its id, for the list and for a selected capture', () => {
    expect(shortTitleId(ENDPOINT_ID)).toBe('x7g…q3');
    expect(titleOf(`/e/${ENDPOINT_ID}`)).toBe('Endpoint x7g…q3 — webhooks.live');
    expect(titleOf(`/e/${ENDPOINT_ID}/c/0193a1c2-9d10-7a3e-8b44-1c2d3e4f5a01`)).toBe('Endpoint x7g…q3 — webhooks.live');
  });

  it('names the workspace section and falls back to Settings without one', () => {
    expect(titleOf(`/e/${ENDPOINT_ID}/settings/general`)).toBe('Settings · General — webhooks.live');
    expect(titleOf(`/e/${ENDPOINT_ID}/settings/rules/0193a1c2`)).toBe('Settings · Rules — webhooks.live');
    expect(titleOf(`/e/${ENDPOINT_ID}/forwarding`)).toBe('Settings · Forwarding — webhooks.live');
    expect(titleOf(`/e/${ENDPOINT_ID}/settings`)).toBe('Settings — webhooks.live');
  });

  it('titles the account and tool routes', () => {
    expect(titleOf('/docs/api')).toBe('API reference — webhooks.live');
    expect(titleOf('/docs/api/tokens_create')).toBe('API reference — webhooks.live');
    expect(titleOf('/send')).toBe('Composer — webhooks.live');
    expect(titleOf('/inbox')).toBe('Inbox — webhooks.live');
    expect(titleOf('/usage')).toBe('Usage — webhooks.live');
    expect(titleOf('/account')).toBe('Account — webhooks.live');
    expect(titleOf('/settings')).toBe('Preferences — webhooks.live');
    expect(titleOf('/settings/tokens')).toBe('API tokens — webhooks.live');
    expect(titleOf('/endpoints/')).toBe('My endpoints — webhooks.live');
  });

  it('keeps the bare site name for the home page', () => {
    expect(titleOf('/')).toBe('webhooks.live');
  });

  it('reports unknown paths, malformed endpoint ids and unknown sections as not found', () => {
    expect(titleOf('/nothing/here')).toBe('Not found — webhooks.live');
    expect(titleOf('/e/short')).toBe('Not found — webhooks.live');
    expect(titleOf(`/e/${ENDPOINT_ID}/settings/nosuch`)).toBe('Not found — webhooks.live');
  });
});

describe('document title with an unread mark', () => {
  afterEach(() => {
    setUnreadTitle(null);
  });

  it('keeps the mark in front of the title when the route title changes', () => {
    setBaseTitle('Endpoint x7g…q3 — webhooks.live');
    setUnreadTitle((title) => `(3) ${title}`);
    expect(document.title).toBe('(3) Endpoint x7g…q3 — webhooks.live');

    setBaseTitle('Inbox — webhooks.live');
    expect(document.title).toBe('(3) Inbox — webhooks.live');

    setUnreadTitle(null);
    expect(document.title).toBe('Inbox — webhooks.live');
  });
});
