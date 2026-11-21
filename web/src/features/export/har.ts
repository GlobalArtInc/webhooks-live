import { bytesToBase64, decodeBody } from './body';
import { DEFAULT_MIME_TYPE, HAR_CREATOR_NAME, HAR_CREATOR_VERSION, HAR_HTTP_VERSION, HAR_VERSION } from './constants';
import { findHeaderValue } from './headers';
import type {
  CaptureExportInput,
  EndpointResponseConfig,
  HarContent,
  HarDocument,
  HarEntry,
  HarNameValue,
  HarOptions,
  HarPostData,
  HarRequest,
  HarResponse,
} from './types';

const NOT_AVAILABLE = -1;

function pairs(source: readonly HarNameValue[]): HarNameValue[] {
  return source.map((item) => ({ name: item.name, value: item.value }));
}

function buildRequest(capture: CaptureExportInput): HarRequest {
  const body = decodeBody(capture.body);
  const request: HarRequest = {
    method: capture.method,
    url: capture.url,
    httpVersion: HAR_HTTP_VERSION,
    cookies: [],
    headers: pairs(capture.headers),
    queryString: pairs(capture.query),
    headersSize: NOT_AVAILABLE,
    bodySize: body.bytes.length,
  };
  if (body.bytes.length > 0) {
    const mimeType = findHeaderValue(capture.headers, 'content-type') ?? DEFAULT_MIME_TYPE;
    const postData: HarPostData =
      body.text === null
        ? { mimeType, text: bytesToBase64(body.bytes), _encoding: 'base64' }
        : { mimeType, text: body.text };
    request.postData = postData;
  }
  return request;
}

function buildConfiguredResponse(capture: CaptureExportInput, config: EndpointResponseConfig): HarResponse {
  const body = decodeBody(config.body);
  const content: HarContent = { size: body.bytes.length, mimeType: config.content_type };
  if (body.bytes.length > 0) {
    if (body.text === null) {
      content.text = bytesToBase64(body.bytes);
      content.encoding = 'base64';
    } else {
      content.text = body.text;
    }
  }
  const headers = pairs(config.headers);
  if (findHeaderValue(headers, 'content-type') === null) {
    headers.unshift({ name: 'Content-Type', value: config.content_type });
  }
  return {
    status: capture.response_status,
    statusText: '',
    httpVersion: HAR_HTTP_VERSION,
    cookies: [],
    headers,
    content,
    redirectURL: '',
    headersSize: NOT_AVAILABLE,
    bodySize: body.bytes.length,
  };
}

function buildResponse(capture: CaptureExportInput, config: EndpointResponseConfig | undefined): HarResponse {
  if (config !== undefined) {
    return buildConfiguredResponse(capture, config);
  }
  return {
    status: capture.response_status,
    statusText: '',
    httpVersion: HAR_HTTP_VERSION,
    cookies: [],
    headers: [],
    content: { size: 0, mimeType: '' },
    redirectURL: '',
    headersSize: NOT_AVAILABLE,
    bodySize: NOT_AVAILABLE,
  };
}

export function toHar(capture: CaptureExportInput, options: HarOptions = {}): HarDocument {
  const entry: HarEntry = {
    startedDateTime: capture.received_at,
    time: 0,
    request: buildRequest(capture),
    response: buildResponse(capture, options.endpointResponse),
    cache: {},
    timings: { send: 0, wait: 0, receive: 0 },
  };
  return {
    log: {
      version: HAR_VERSION,
      creator: { name: HAR_CREATOR_NAME, version: HAR_CREATOR_VERSION },
      entries: [entry],
    },
  };
}
