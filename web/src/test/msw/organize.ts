import { http, type HttpHandler } from 'msw';
import type { Endpoint, Folder, User } from '../../api/types';
import { emptyResponse, errorResponse, jsonResponse, readJsonBody } from './responses';
import { mockStore, randomUuid } from './store';

interface FolderPath {
  folderId: string;
}

interface FolderEndpointPath extends FolderPath {
  endpointId: string;
}

interface EndpointPath {
  endpointId: string;
}

interface NameBody {
  name?: unknown;
  position?: unknown;
}

const MAX_FOLDER_NAME_LENGTH = 64;

function signInRequired(): Response {
  return errorResponse(401, 'unauthenticated', 'Sign in to continue.');
}

function signedInUser(): User | null {
  return mockStore.instance.user;
}

function isVisible(endpoint: Endpoint, userId: string, role: string | null): boolean {
  const owned = endpoint.owner?.id === userId;
  const shared = (mockStore.members.get(endpoint.id) ?? []).some((member) => member.user.id === userId);
  if (role === 'owner') {
    return owned;
  }
  if (role === 'shared') {
    return shared;
  }
  return owned || shared;
}

function listAccountEndpoints(params: URLSearchParams, userId: string): Endpoint[] {
  const tag = params.get('tag');
  const folderId = params.get('folder');
  const folder = folderId === null ? null : mockStore.folders.find((item) => item.id === folderId);
  if (folderId !== null && folder === undefined) {
    return [];
  }
  return [...mockStore.endpoints.values()]
    .filter((endpoint) => isVisible(endpoint, userId, params.get('role')))
    .filter((endpoint) => tag === null || endpoint.tags.includes(tag))
    .filter((endpoint) => folder === null || folder === undefined || folder.endpoint_ids.includes(endpoint.id))
    .sort((left, right) => right.last_active_at.localeCompare(left.last_active_at));
}

function folderNameProblem(name: string): Response | null {
  if (name === '') {
    return errorResponse(422, 'validation_failed', 'name is required', [{ field: 'name', message: 'is required' }]);
  }
  if (Array.from(name).length > MAX_FOLDER_NAME_LENGTH) {
    return errorResponse(422, 'validation_failed', 'name is too long', [
      { field: 'name', message: 'must be at most 64 characters' },
    ]);
  }
  return null;
}

function nameTaken(name: string, exceptId: string | null): boolean {
  const lowered = name.toLowerCase();
  return mockStore.folders.some((folder) => folder.id !== exceptId && folder.name.toLowerCase() === lowered);
}

function folderNamed(folderId: string): Folder | undefined {
  return mockStore.folders.find((folder) => folder.id === folderId);
}

function replaceFolder(updated: Folder): void {
  mockStore.folders = mockStore.folders.map((folder) => (folder.id === updated.id ? updated : folder));
}

function namedText(body: NameBody): string | null {
  return typeof body.name === 'string' ? body.name.trim() : null;
}

export const organizeHandlers: HttpHandler[] = [
  http.get('/api/v1/endpoints', ({ request }) => {
    if (mockStore.instance.auth_mode === 'open') {
      return errorResponse(403, 'not_available_in_mode', 'This route needs sso or mixed mode.');
    }
    const user = signedInUser();
    if (user === null) {
      return signInRequired();
    }
    const items = listAccountEndpoints(new URL(request.url).searchParams, user.id);
    return jsonResponse({ items, next_cursor: null });
  }),

  http.get('/api/v1/folders', () => {
    if (signedInUser() === null) {
      return signInRequired();
    }
    return jsonResponse({ items: mockStore.folders });
  }),

  http.post('/api/v1/folders', async ({ request }) => {
    if (signedInUser() === null) {
      return signInRequired();
    }
    const body = await readJsonBody<NameBody>(request);
    const name = body === null ? null : namedText(body);
    if (name === null) {
      return errorResponse(400, 'invalid_json', 'Request body is not valid JSON.');
    }
    const problem = folderNameProblem(name);
    if (problem !== null) {
      return problem;
    }
    if (nameTaken(name, null)) {
      return errorResponse(409, 'name_taken', 'A folder with this name already exists.');
    }
    const folder: Folder = {
      id: randomUuid(),
      name,
      position: mockStore.folders.length,
      endpoint_ids: [],
      created_at: new Date().toISOString(),
    };
    mockStore.folders = [...mockStore.folders, folder];
    return jsonResponse(folder, 201);
  }),

  http.patch<FolderPath>('/api/v1/folders/:folderId', async ({ params, request }) => {
    const existing = folderNamed(params.folderId);
    if (existing === undefined) {
      return errorResponse(404, 'folder_not_found', 'Folder not found.');
    }
    const body = await readJsonBody<NameBody>(request);
    if (body === null) {
      return errorResponse(400, 'invalid_json', 'Request body is not valid JSON.');
    }
    const name = body.name === undefined ? existing.name : namedText(body);
    if (name === null) {
      return errorResponse(400, 'invalid_json', 'Request body is not valid JSON.');
    }
    const problem = folderNameProblem(name);
    if (problem !== null) {
      return problem;
    }
    if (nameTaken(name, existing.id)) {
      return errorResponse(409, 'name_taken', 'A folder with this name already exists.');
    }
    const updated: Folder = { ...existing, name };
    replaceFolder(updated);
    return jsonResponse(updated);
  }),

  http.delete<FolderPath>('/api/v1/folders/:folderId', ({ params }) => {
    if (folderNamed(params.folderId) === undefined) {
      return errorResponse(404, 'folder_not_found', 'Folder not found.');
    }
    mockStore.folders = mockStore.folders.filter((folder) => folder.id !== params.folderId);
    return emptyResponse();
  }),

  http.put<FolderEndpointPath>('/api/v1/folders/:folderId/endpoints/:endpointId', ({ params }) => {
    const user = signedInUser();
    if (user === null) {
      return signInRequired();
    }
    const folder = folderNamed(params.folderId);
    if (folder === undefined) {
      return errorResponse(404, 'folder_not_found', 'Folder not found.');
    }
    const endpoint = mockStore.endpoints.get(params.endpointId);
    if (endpoint === undefined || !isVisible(endpoint, user.id, null)) {
      return errorResponse(404, 'endpoint_not_found', 'Endpoint not found.');
    }
    for (const other of mockStore.folders) {
      if (other.id !== folder.id && other.endpoint_ids.includes(params.endpointId)) {
        replaceFolder({ ...other, endpoint_ids: other.endpoint_ids.filter((id) => id !== params.endpointId) });
      }
    }
    const current = folderNamed(params.folderId) ?? folder;
    if (!current.endpoint_ids.includes(params.endpointId)) {
      replaceFolder({ ...current, endpoint_ids: [...current.endpoint_ids, params.endpointId] });
    }
    return emptyResponse();
  }),

  http.delete<FolderEndpointPath>('/api/v1/folders/:folderId/endpoints/:endpointId', ({ params }) => {
    const folder = folderNamed(params.folderId);
    if (folder === undefined) {
      return errorResponse(404, 'folder_not_found', 'Folder not found.');
    }
    replaceFolder({ ...folder, endpoint_ids: folder.endpoint_ids.filter((id) => id !== params.endpointId) });
    return emptyResponse();
  }),

  http.post<EndpointPath>('/api/v1/endpoints/:endpointId/claim', ({ params }) => {
    const user = signedInUser();
    if (user === null) {
      return errorResponse(403, 'sign_in_required', 'Sign in to continue.');
    }
    const endpoint = mockStore.endpoints.get(params.endpointId);
    if (endpoint === undefined) {
      return errorResponse(410, 'endpoint_gone', 'Endpoint is gone.');
    }
    if (endpoint.owner?.id === user.id) {
      return errorResponse(409, 'already_owned', 'Endpoint is already yours.');
    }
    if (endpoint.owner !== null) {
      return errorResponse(410, 'endpoint_gone', 'Endpoint is gone.');
    }
    const claimed: Endpoint = { ...endpoint, owner: user, my_role: 'owner' };
    mockStore.endpoints.set(claimed.id, claimed);
    return jsonResponse(claimed);
  }),
];
