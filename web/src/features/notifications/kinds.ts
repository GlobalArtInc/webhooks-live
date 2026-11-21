import { CHANNEL_KINDS, type ChannelKind } from './types';

export type ConfigField = 'url' | 'tokenSecret' | 'chatId' | 'addressId';

const KIND_FIELDS: Readonly<Record<ChannelKind, readonly ConfigField[]>> = {
  webhook: ['url'],
  slack: ['url'],
  discord: ['url'],
  telegram: ['tokenSecret', 'chatId'],
  email: ['addressId'],
  push: [],
};

const ACCOUNT_KINDS: ReadonlySet<ChannelKind> = new Set<ChannelKind>(['email', 'push']);

export function fieldsOf(kind: ChannelKind): readonly ConfigField[] {
  return KIND_FIELDS[kind];
}

export function isChannelKind(value: string): value is ChannelKind {
  return (CHANNEL_KINDS as readonly string[]).includes(value);
}

export function needsAccount(kind: ChannelKind): boolean {
  return ACCOUNT_KINDS.has(kind);
}

export function kindsOffered(features: readonly string[] | undefined, signedIn: boolean): ChannelKind[] {
  const offered = features ?? [];
  return CHANNEL_KINDS.filter((kind) => offered.includes(kind) && (signedIn || !needsAccount(kind)));
}
