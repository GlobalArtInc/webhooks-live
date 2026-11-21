import common from './en/common.json';
import detail from './en/detail.json';
import diff from './en/diff.json';
import editor from './en/editor.json';
import endpoint from './en/endpoint.json';
import exportMessages from './en/export.json';
import forwarding from './en/forwarding.json';
import home from './en/home.json';
import live from './en/live.json';
import sample from './en/sample.json';
import schedules from './en/schedules.json';
import state from './en/state.json';
import workspace from './en/workspace.json';

export type SourceMessages = typeof common &
  typeof detail &
  typeof diff &
  typeof editor &
  typeof endpoint &
  typeof exportMessages &
  typeof forwarding &
  typeof home &
  typeof live &
  typeof sample &
  typeof schedules &
  typeof state &
  typeof workspace;

export type SourceKey = keyof SourceMessages;
