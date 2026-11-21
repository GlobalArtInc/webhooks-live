export type DetailSectionId =
  'summary' | 'forward' | 'response' | 'rules' | 'query' | 'form' | 'body' | 'headers' | 'dns';

export type CaptureDetailState = 'loading' | 'ready' | 'large-pending' | 'deleted' | 'error';

export interface FormFileView {
  name: string;
  filename: string | null;
  content_type: string | null;
  size: number;
}
