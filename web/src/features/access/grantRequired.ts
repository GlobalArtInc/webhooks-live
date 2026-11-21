import { ApiError } from '../../api/client';

export const GRANT_REQUIRED_CODE = 'grant_required';

export function isGrantRequired(error: unknown): boolean {
  return error instanceof ApiError && error.status === 401 && error.code === GRANT_REQUIRED_CODE;
}
