import common from './en/common.json';
import detail from './en/detail.json';
import diff from './en/diff.json';
import exportMessages from './en/export.json';
import home from './en/home.json';
import sample from './en/sample.json';
import state from './en/state.json';

export type SourceMessages = typeof common &
  typeof detail &
  typeof diff &
  typeof exportMessages &
  typeof home &
  typeof sample &
  typeof state;

export type SourceKey = keyof SourceMessages;
