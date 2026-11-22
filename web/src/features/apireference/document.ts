import { useQuery } from '@tanstack/react-query';
import { requestJson } from '../../api/client';
import { shouldRetryRequest } from '../../api/queries';
import { isObject, objectOf, stringOf, type JsonObject } from './json';

export interface OpenApiDocument extends JsonObject {
  openapi: string;
  paths: JsonObject;
}

export class InvalidDocumentError extends Error {
  constructor(message: string) {
    super(message);
    this.name = 'InvalidDocumentError';
  }
}

export const OPENAPI_PATH = '/openapi.json';
export const OPENAPI_URL = `/api/v1${OPENAPI_PATH}`;
const DOCUMENT_STALE_MS = 60 * 60 * 1000;
const SUPPORTED_VERSION = /^3\.\d+(\.\d+)?/;

export const openApiQueryKey = ['openapi', 'document'] as const;

export function parseOpenApiDocument(value: unknown): OpenApiDocument {
  if (!isObject(value)) {
    throw new InvalidDocumentError('The OpenAPI document is not an object');
  }
  const version = stringOf(value.openapi);
  if (version === null || !SUPPORTED_VERSION.test(version)) {
    throw new InvalidDocumentError('The document is not an OpenAPI 3 document');
  }
  if (!isObject(value.paths)) {
    throw new InvalidDocumentError('The OpenAPI document has no paths');
  }
  return { ...value, openapi: version, paths: objectOf(value.paths) };
}

export async function getOpenApiDocument(): Promise<OpenApiDocument> {
  const payload = await requestJson<unknown>(OPENAPI_PATH, { method: 'GET' });
  return parseOpenApiDocument(payload);
}

export function useOpenApiDocument() {
  return useQuery({
    queryKey: openApiQueryKey,
    queryFn: getOpenApiDocument,
    staleTime: DOCUMENT_STALE_MS,
    retry: (count, error) => !(error instanceof InvalidDocumentError) && shouldRetryRequest(count, error),
  });
}
