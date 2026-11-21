import { isCaptureId } from '../../app/ids';
import { validatePreviewSearch, type PreviewSearch } from '../templating/previewSearch';

export interface SectionSearch extends PreviewSearch {
  test?: 'all';
  capture?: string;
}

export function validateSectionSearch(search: Record<string, unknown>): SectionSearch {
  const result: SectionSearch = validatePreviewSearch(search);
  if (search.test === 'all') {
    result.test = 'all';
  }
  if (typeof search.capture === 'string' && isCaptureId(search.capture)) {
    result.capture = search.capture;
  }
  return result;
}
