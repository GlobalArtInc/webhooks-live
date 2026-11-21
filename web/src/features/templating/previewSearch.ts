import { isCaptureId } from '../../app/ids';

export interface PreviewSearch {
  preview?: string;
}

export function validatePreviewSearch(search: Record<string, unknown>): PreviewSearch {
  const preview = search.preview;
  return typeof preview === 'string' && isCaptureId(preview) ? { preview } : {};
}
