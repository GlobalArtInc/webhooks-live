import type { FieldError } from '../../api/types';
import { fieldsOf, type ConfigField } from './kinds';
import type { Channel, ChannelConfig, ChannelInput, ChannelKind } from './types';

export const DEFAULT_PER_HOUR = 60;
export const NAME_MAX = 64;

export type IssueField = 'name' | 'perHour' | ConfigField;

export type IssueCode =
  | 'required'
  | 'nameTooLong'
  | 'perHour'
  | 'targetRefused'
  | 'hostNotAllowed'
  | 'invalidFormat'
  | 'notVerified'
  | 'unknown';

export type IssueMap = ReadonlyMap<IssueField, IssueCode>;

export const NO_ISSUES: IssueMap = new Map<IssueField, IssueCode>();

export interface ChannelDraft {
  name: string;
  kind: ChannelKind;
  perHour: string;
  url: string;
  tokenSecret: string;
  chatId: string;
  addressId: string;
}

const SERVER_FIELDS: ReadonlyMap<string, IssueField> = new Map<string, IssueField>([
  ['name', 'name'],
  ['per_hour', 'perHour'],
  ['config.url', 'url'],
  ['config.token_secret', 'tokenSecret'],
  ['config.chat_id', 'chatId'],
  ['config.address_id', 'addressId'],
]);

const SERVER_CODES: ReadonlyMap<string, IssueCode> = new Map<string, IssueCode>([
  ['target_refused', 'targetRefused'],
  ['host_not_allowed', 'hostNotAllowed'],
  ['invalid_format', 'invalidFormat'],
  ['not_verified', 'notVerified'],
]);

export function blankDraft(kind: ChannelKind): ChannelDraft {
  return {
    name: '',
    kind,
    perHour: String(DEFAULT_PER_HOUR),
    url: '',
    tokenSecret: '',
    chatId: '',
    addressId: '',
  };
}

export function draftOf(channel: Channel): ChannelDraft {
  return {
    name: channel.name,
    kind: channel.kind,
    perHour: String(channel.per_hour),
    url: '',
    tokenSecret: channel.config.token_secret ?? '',
    chatId: channel.config.chat_id ?? '',
    addressId: channel.config.address_id ?? '',
  };
}

export function serializeDraft(draft: ChannelDraft): string {
  return JSON.stringify(draft);
}

function valueOf(draft: ChannelDraft, field: Exclude<ConfigField, 'url'>): string {
  switch (field) {
    case 'tokenSecret':
      return draft.tokenSecret;
    case 'chatId':
      return draft.chatId;
    case 'addressId':
      return draft.addressId;
  }
}

export function validateDraft(draft: ChannelDraft, isNew: boolean): IssueMap {
  const issues = new Map<IssueField, IssueCode>();
  const name = draft.name.trim();
  if (name === '') {
    issues.set('name', 'required');
  } else if (name.length > NAME_MAX) {
    issues.set('name', 'nameTooLong');
  }
  const perHour = Number(draft.perHour);
  if (!Number.isInteger(perHour) || perHour < 1) {
    issues.set('perHour', 'perHour');
  }
  for (const field of fieldsOf(draft.kind)) {
    if (field === 'url') {
      if (isNew && draft.url.trim() === '') {
        issues.set(field, 'required');
      }
    } else if (valueOf(draft, field).trim() === '') {
      issues.set(field, 'required');
    }
  }
  return issues;
}

function configOf(draft: ChannelDraft, isNew: boolean): ChannelConfig | null {
  switch (draft.kind) {
    case 'webhook':
    case 'slack':
    case 'discord': {
      const url = draft.url.trim();
      return url === '' ? null : { url };
    }
    case 'telegram':
      return { token_secret: draft.tokenSecret, chat_id: draft.chatId.trim() };
    case 'email':
      return { address_id: draft.addressId };
    case 'push':
      return isNew ? {} : null;
  }
}

export function channelInputOf(draft: ChannelDraft, isNew: boolean): ChannelInput {
  const input: ChannelInput = { name: draft.name.trim(), per_hour: Number(draft.perHour) };
  if (isNew) {
    input.kind = draft.kind;
    input.trigger = 'capture';
    input.enabled = true;
  }
  const config = configOf(draft, isNew);
  if (config !== null) {
    input.config = config;
  }
  return input;
}

export function issuesFromServer(details: readonly FieldError[]): IssueMap {
  const issues = new Map<IssueField, IssueCode>();
  for (const detail of details) {
    const field = SERVER_FIELDS.get(detail.field);
    if (field !== undefined) {
      issues.set(field, SERVER_CODES.get(detail.code ?? '') ?? 'unknown');
    }
  }
  return issues;
}
