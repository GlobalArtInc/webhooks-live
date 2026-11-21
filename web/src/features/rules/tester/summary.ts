import type { TFunction } from 'i18next';
import { ApiError } from '../../../api/client';
import { formatByteSize } from '../../../components/format/format';
import { SUMMARY_CHARS } from './constants';
import { clip, UnreadableResultError } from './parse';
import type { JsonObject, JsonValue, TraceItem } from './types';

const FIELD_LIMIT = 5;

export interface FailureView {
  message: string;
  fields: string[];
}

function asRecord(value: JsonValue | null): JsonObject | null {
  return typeof value === 'object' && value !== null && !Array.isArray(value) ? value : null;
}

function numberOf(value: JsonValue | undefined): number {
  return typeof value === 'number' && Number.isFinite(value) ? value : 0;
}

function textOf(value: JsonValue | undefined): string {
  return typeof value === 'string' ? value : '';
}

export function valueSummary(value: JsonValue | undefined): string {
  if (value === undefined) {
    return '';
  }
  return clip(typeof value === 'string' ? value : (JSON.stringify(value) ?? ''), SUMMARY_CHARS);
}

export function outputSummary(item: TraceItem, t: TFunction, locale: string): string {
  const output = asRecord(item.output);
  if (output !== null && item.stepKind === 'respond') {
    const contentType = textOf(output.content_type);
    return t('wl.tester.summary.respond', {
      count: numberOf(output.headers),
      status: numberOf(output.status),
      contentType: contentType === '' ? t('wl.tester.summary.noType') : contentType,
      size: formatByteSize(numberOf(output.body_bytes), locale),
    });
  }
  if (output !== null && typeof output.into === 'string') {
    if (output.into === 'payload') {
      return t('wl.tester.summary.payload', { size: formatByteSize(numberOf(output.bytes), locale) });
    }
    return `var.${output.into} = ${valueSummary(output.value)}`;
  }
  if (output !== null && item.stepKind === 'script') {
    const names = Array.isArray(output.vars) ? output.vars.length : 0;
    return t('wl.tester.summary.script', { count: names, operations: numberOf(output.operations) });
  }
  if (item.detail !== null) {
    return clip(item.detail, SUMMARY_CHARS);
  }
  return output === null ? '' : valueSummary(output);
}

export function scriptLog(item: TraceItem): string[] {
  const output = asRecord(item.output);
  const log = output === null ? '' : textOf(output.log);
  return log === '' ? [] : log.split('\n').slice(0, 20);
}

export function describeFailure(error: unknown, t: TFunction): FailureView {
  if (error instanceof ApiError) {
    return {
      message: error.message,
      fields: error.details
        .slice(0, FIELD_LIMIT)
        .map((detail) => `${clip(detail.field, 120)}: ${clip(detail.message, 240)}`),
    };
  }
  if (error instanceof UnreadableResultError) {
    return { message: t('wl.tester.error.unreadable'), fields: [] };
  }
  if (error instanceof Error) {
    return { message: error.message, fields: [] };
  }
  return { message: t('wl.tester.error.unknown'), fields: [] };
}
