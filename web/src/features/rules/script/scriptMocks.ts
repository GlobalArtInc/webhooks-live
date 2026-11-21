import { http, HttpResponse, type HttpHandler } from 'msw';
import { SCRIPT_META_FIXTURE, fixtureValidate } from './fixtures';

export const SCRIPT_META_URL = '/api/v1/script/meta';

export const SCRIPT_VALIDATE_URL = '/api/v1/script/validate';

interface ValidateBody {
  source?: unknown;
}

export const scriptHandlers: HttpHandler[] = [
  http.get(SCRIPT_META_URL, () => HttpResponse.json(SCRIPT_META_FIXTURE)),
  http.post(SCRIPT_VALIDATE_URL, async ({ request }) => {
    const body = (await request.json()) as ValidateBody;
    if (typeof body.source !== 'string') {
      return HttpResponse.json({ error: { code: 'invalid_request', message: 'source is required' } }, { status: 400 });
    }
    return HttpResponse.json({ diagnostics: fixtureValidate(body.source) });
  }),
];
