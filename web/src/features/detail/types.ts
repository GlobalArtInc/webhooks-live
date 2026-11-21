export type DetailSectionId = 'summary' | 'forward' | 'query' | 'form' | 'body' | 'headers';

export type CaptureDetailState = 'loading' | 'ready' | 'large-pending' | 'deleted' | 'error';

export interface FormFileView {
  name: string;
  filename: string | null;
  content_type: string | null;
  size: number;
}
