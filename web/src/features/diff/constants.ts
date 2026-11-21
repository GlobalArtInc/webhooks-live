export const KIB = 1024;
export const MIB = KIB * KIB;
export const LARGE_BODY_LIMIT = MIB;

export const NARROW_VIEWPORT_QUERY = '(max-width: 767px)';

export const VOLATILE_HEADER_NAMES: readonly string[] = [
  'connection',
  'content-length',
  'date',
  'traceparent',
  'tracestate',
  'via',
  'x-amzn-trace-id',
  'x-forwarded-*',
  'x-request-id',
  'cf-*',
];
