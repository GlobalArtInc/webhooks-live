import { TOKEN_SCOPES, type ApiTokenInput, type TokenScope } from '../../api/tokens';

export type EndpointMode = 'all' | 'only';

export type NameProblem = 'required' | 'tooLong' | 'control';
export type EndpointsProblem = 'required' | 'tooMany';

export interface TokenDraft {
  name: string;
  scopes: TokenScope[];
  endpointMode: EndpointMode;
  endpointIds: string[];
  days: number;
}

export interface DraftProblems {
  name?: NameProblem;
  scopes?: 'required';
  endpoints?: EndpointsProblem;
}

export const EXPIRY_PRESETS: readonly number[] = [7, 30, 90, 180, 365];
export const DEFAULT_EXPIRY_DAYS = 90;
export const MAX_NAME_CHARS = 64;
export const MAX_ENDPOINTS = 50;
export const YEAR_DAYS = 365;

const MILLISECONDS_PER_DAY = 86_400_000;
const CONTROL_CHARACTER = /\p{Cc}/u;

export function emptyDraft(): TokenDraft {
  return { name: '', scopes: ['read'], endpointMode: 'all', endpointIds: [], days: DEFAULT_EXPIRY_DAYS };
}

export function toggleScope(scopes: readonly TokenScope[], scope: TokenScope, on: boolean): TokenScope[] {
  const without = scopes.filter((existing) => existing !== scope);
  const next = on ? [...without, scope] : without;
  return TOKEN_SCOPES.filter((candidate) => next.includes(candidate));
}

export function toggleEndpoint(ids: readonly string[], id: string, on: boolean): string[] {
  const without = ids.filter((existing) => existing !== id);
  return on ? [...without, id] : without;
}

export function pruneEndpointIds(draft: TokenDraft, known: ReadonlySet<string>): TokenDraft {
  return { ...draft, endpointIds: draft.endpointIds.filter((id) => known.has(id)) };
}

export function writesEndpointsWhileRestricted(draft: TokenDraft): boolean {
  return draft.endpointMode === 'only' && draft.scopes.includes('endpoints:write');
}

function nameProblem(name: string): NameProblem | undefined {
  const trimmed = name.trim();
  if (trimmed === '') {
    return 'required';
  }
  if ([...trimmed].length > MAX_NAME_CHARS) {
    return 'tooLong';
  }
  return CONTROL_CHARACTER.test(trimmed) ? 'control' : undefined;
}

function endpointsProblem(draft: TokenDraft): EndpointsProblem | undefined {
  if (draft.endpointMode === 'all') {
    return undefined;
  }
  if (draft.endpointIds.length === 0) {
    return 'required';
  }
  return draft.endpointIds.length > MAX_ENDPOINTS ? 'tooMany' : undefined;
}

export function validateDraft(draft: TokenDraft): DraftProblems {
  const problems: DraftProblems = {};
  const name = nameProblem(draft.name);
  if (name !== undefined) {
    problems.name = name;
  }
  if (draft.scopes.length === 0) {
    problems.scopes = 'required';
  }
  const endpoints = endpointsProblem(draft);
  if (endpoints !== undefined) {
    problems.endpoints = endpoints;
  }
  return problems;
}

export function hasProblems(problems: DraftProblems): boolean {
  return problems.name !== undefined || problems.scopes !== undefined || problems.endpoints !== undefined;
}

export function draftToInput(draft: TokenDraft): ApiTokenInput {
  const input: ApiTokenInput = {
    name: draft.name.trim(),
    scopes: draft.scopes,
    expires_in_days: draft.days,
  };
  if (draft.endpointMode === 'only') {
    input.endpoint_ids = draft.endpointIds;
  }
  return input;
}

export function expiryDate(days: number, now: number): Date {
  return new Date(now + days * MILLISECONDS_PER_DAY);
}
