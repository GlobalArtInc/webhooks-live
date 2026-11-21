import { useEffect, useState } from 'react';
import type { Body } from '../../api/types';
import { sha256Hex } from '../body/hash';
import type { Bytes } from '../body/types';

interface DigestResult {
  source: string;
  hex: string;
}

function decodeBase64(data: string): Bytes {
  return Uint8Array.from(atob(data), (character) => character.charCodeAt(0));
}

export function useBodyDigest(body: Body, enabled: boolean): string | null {
  const [result, setResult] = useState<DigestResult | null>(null);

  useEffect(() => {
    if (!enabled || body.encoding !== 'base64') {
      return undefined;
    }
    let cancelled = false;
    const source = body.data;
    Promise.resolve()
      .then(() => sha256Hex(decodeBase64(source)))
      .then(
        (hex) => {
          if (!cancelled) {
            setResult({ source, hex });
          }
        },
        () => undefined,
      );
    return () => {
      cancelled = true;
    };
  }, [body.data, body.encoding, enabled]);

  return result !== null && result.source === body.data && enabled ? result.hex : null;
}
