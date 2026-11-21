import type { Ref } from 'react';

export type Bytes = Uint8Array<ArrayBuffer>;

export type ImageType = 'png' | 'jpeg' | 'gif' | 'webp';
export type TextKind = 'json' | 'xml' | 'text' | 'html';

export interface TextBodyClass {
  kind: TextKind;
}

export interface BinaryBodyClass {
  kind: 'binary';
}

export interface ImageBodyClass {
  kind: 'image';
  image: ImageType;
}

export type LargeHint = TextBodyClass | BinaryBodyClass | ImageBodyClass;

export interface LargeBodyClass {
  kind: 'large';
  hint: LargeHint;
}

export type BodyClass = LargeHint | LargeBodyClass;

export interface BytesBodyContent {
  type: 'bytes';
  bytes: Bytes;
}

export interface TextBodyContent {
  type: 'text';
  text: string;
}

export type BodyContent = BytesBodyContent | TextBodyContent;

export type BodyLoader = (limit: number | null) => Promise<Bytes>;

export type BodyViewMode = 'pretty' | 'raw';

export type FormatKind = 'json' | 'xml';

export interface FormatError {
  message: string;
  line: number;
  column: number;
}

export interface FormatSuccess {
  ok: true;
  text: string;
}

export interface FormatFailure {
  ok: false;
  error: FormatError;
}

export type FormatResult = FormatSuccess | FormatFailure;

export interface FormatTask {
  kind: FormatKind;
  bytes: Bytes;
}

export interface FormatRequest extends FormatTask {
  id: number;
}

export interface FormatResponse {
  id: number;
  result: FormatResult;
}

export interface BodyFormatter {
  format: (task: FormatTask) => Promise<FormatResult>;
  dispose: () => void;
}

export interface BodyViewerProps {
  bodyId: string;
  body?: BodyContent;
  loadBody?: BodyLoader;
  contentType: string;
  size: number;
  mode: BodyViewMode;
  wrap: boolean;
  search: string;
  onModeChange: (mode: BodyViewMode) => void;
  onWrapChange: (wrap: boolean) => void;
  onSearchChange: (search: string) => void;
  onDownloadRaw: () => void;
  formatter?: BodyFormatter;
}

export type CodeLanguage = 'json' | 'xml' | 'html' | 'plain';

export interface EditorConfig {
  text: string;
  language: CodeLanguage;
  wrap: boolean;
  search: string;
  label: string;
}

export interface EditorHandle {
  update: (config: EditorConfig) => void;
  findNext: () => void;
  findPrevious: () => void;
  destroy: () => void;
}

export interface CodeViewerHandle {
  findNext: () => void;
  findPrevious: () => void;
}

export interface CodeViewerProps extends EditorConfig {
  ref?: Ref<CodeViewerHandle>;
}
