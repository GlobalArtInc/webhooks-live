import common from './en/common.json';
import sample from './en/sample.json';
import state from './en/state.json';

export type SourceMessages = typeof common & typeof sample & typeof state;

export type SourceKey = keyof SourceMessages;
