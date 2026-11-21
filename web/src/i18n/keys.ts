import common from './en/common.json';
import detail from './en/detail.json';
import exportMessages from './en/export.json';
import sample from './en/sample.json';
import state from './en/state.json';

export type SourceMessages = typeof common & typeof detail & typeof exportMessages & typeof sample & typeof state;

export type SourceKey = keyof SourceMessages;
