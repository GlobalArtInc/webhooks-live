import { createKeyValueRow } from '../../components/KeyValueEditor/rows';
import type { ComposerSeed } from '../composer';
import type { OperationSample } from './samples';

export function composerSeedFor(sample: OperationSample): ComposerSeed {
  return {
    draft: {
      method: sample.method,
      url: sample.url,
      headers: sample.headers.map((header) => createKeyValueRow(header.name, header.value)),
      body: { encoding: 'utf8', data: sample.body ?? '', truncated: false },
      timeout: '',
      followRedirects: false,
    },
    banner: null,
    sourceCaptureId: null,
  };
}
