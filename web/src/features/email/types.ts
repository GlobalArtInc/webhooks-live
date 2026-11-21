import type { Capture, Timestamp } from '../../api/types';

export type PartDisposition = 'inline' | 'attachment';

export interface EmailPart {
  index: number;
  content_type: string;
  filename: string | null;
  size: number;
  disposition: PartDisposition | null;
  is_text: boolean;
}

export interface ParseIssue {
  detail: string | null;
}

export interface EmailMeta {
  envelope_from: string | null;
  envelope_to: string[];
  helo: string | null;
  tls: boolean;
  tag: string | null;
  subject: string | null;
  from: string[];
  to: string[];
  cc: string[];
  date: Timestamp | null;
  message_id: string | null;
  parts: EmailPart[];
  parse_error: ParseIssue | null;
  parts_truncated: boolean;
}

export interface EmailView {
  capture: Capture;
  meta: EmailMeta;
}

export interface InertSegment {
  text: string;
  escaped: boolean;
}
