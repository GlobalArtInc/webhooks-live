const ENDPOINT_ID_PATTERN = /^[0-9a-hjkmnp-tv-z]{24}$/;
const CAPTURE_ID_PATTERN = /^[0-9a-f]{8}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{12}$/;

export function isEndpointId(value: string): boolean {
  return ENDPOINT_ID_PATTERN.test(value);
}

export function isCaptureId(value: string): boolean {
  return CAPTURE_ID_PATTERN.test(value);
}
