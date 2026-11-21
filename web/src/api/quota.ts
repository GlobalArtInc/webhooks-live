import { ApiError } from './client';

const QUOTA_STATUS = 429;
const STORAGE_STATUS = 507;
const COUNT_STATUS = 422;

export function isLimitFailure(error: unknown): boolean {
  if (!(error instanceof ApiError)) {
    return false;
  }
  if (error.code === 'quota_exceeded') {
    return error.status === QUOTA_STATUS || error.status === STORAGE_STATUS;
  }
  return error.code === 'limit_reached' && error.status === COUNT_STATUS;
}
