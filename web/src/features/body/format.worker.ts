import { decodeText } from './bytes';
import { formatDocument } from './format';
import { isFormatRequest } from './guards';
import type { FormatResponse } from './types';

self.addEventListener('message', (event: MessageEvent<unknown>) => {
  const request = event.data;
  if (!isFormatRequest(request)) {
    return;
  }
  const response: FormatResponse = {
    id: request.id,
    result: formatDocument(request.kind, decodeText(request.bytes, false)),
  };
  self.postMessage(response);
});
