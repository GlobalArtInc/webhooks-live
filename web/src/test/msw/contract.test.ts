import { describe, expect, it } from 'vitest';
import { endpointFixture, ENDPOINT_ID } from '../fixtures/endpoint';
import { missingFieldsOf } from '../openapiShapes';
import { registerMockServer } from './server';
import { newEndpoint, seedEndpoint } from './store';

registerMockServer();

const CLONE_URL = `${window.location.origin}/api/v1/endpoints/${ENDPOINT_ID}/clone`;

function postClone(body: unknown): Promise<Response> {
  return fetch(CLONE_URL, {
    method: 'POST',
    headers: { 'Content-Type': 'application/json' },
    body: JSON.stringify(body),
  });
}

describe('mock server contract', () => {
  it('builds endpoints with every field the real Endpoint requires', () => {
    expect(missingFieldsOf('Endpoint', { ...newEndpoint({}) })).toEqual([]);
  });

  it('refuses the clone fields the real server does not know, like the real server', async () => {
    seedEndpoint(endpointFixture, []);

    const response = await postClone({ title: 'copy', parts: ['response'] });

    expect(response.status).toBe(422);
    const body: unknown = await response.json();
    expect(JSON.stringify(body)).toContain('parts');
  });

  it('answers a clone with every field of the real CloneResult', async () => {
    seedEndpoint(endpointFixture, []);

    const response = await postClone({ title: 'copy', include: ['response', 'rules'] });

    expect(response.status).toBe(201);
    const body: Record<string, unknown> = await response.json();
    expect(missingFieldsOf('CloneResult', body)).toEqual([]);
    expect(missingFieldsOf('Endpoint', body)).toEqual([]);
    expect(body.skipped).toEqual(['forward_rules', 'channels', 'schedules']);
  });
});
