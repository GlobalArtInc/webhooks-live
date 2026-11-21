import type { NameValue } from '../../api/types';

export type DiffLayout = 'split' | 'unified';

export type DiffField = 'method' | 'path' | 'query';

export type BodyComparison = 'same' | 'different' | 'not-compared';

export interface NormaliseOptions {
  hideVolatile: boolean;
  sortHeaders: boolean;
  sortQuery: boolean;
  sortJsonKeys: boolean;
  binaryDigest: string | null;
}

export interface NormalisedSummary {
  method: string;
  path: string;
  query: string;
}

export interface NormalisedCapture {
  summary: NormalisedSummary;
  headers: string;
  headerRows: NameValue[];
  body: string;
  binary: boolean;
}

export interface FieldEquality {
  method: boolean;
  path: boolean;
  query: boolean;
}

export interface HeaderCounts {
  added: number;
  removed: number;
  changed: number;
  same: number;
  hidden: number;
}

export interface StructuralSummary {
  fields: FieldEquality;
  headers: HeaderCounts;
  body: BodyComparison;
}

export interface MergeEditorConfig {
  layout: DiffLayout;
  original: string;
  modified: string;
  labelOriginal: string;
  labelModified: string;
  labelUnified: string;
}

export interface MergeEditorHandle {
  destroy: () => void;
}
