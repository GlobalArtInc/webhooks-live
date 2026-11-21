import common from './en/common.json';
import detail from './en/detail.json';
import diff from './en/diff.json';
import editor from './en/editor.json';
import endpoint from './en/endpoint.json';
import exportMessages from './en/export.json';
import exportDialog from './en/exportdialog.json';
import forwarding from './en/forwarding.json';
import home from './en/home.json';
import kinds from './en/kinds.json';
import live from './en/live.json';
import rules from './en/rules.json';
import sample from './en/sample.json';
import script from './en/script.json';
import selection from './en/selection.json';
import state from './en/state.json';
import workspace from './en/workspace.json';

export type SourceMessages = typeof common &
  typeof detail &
  typeof diff &
  typeof editor &
  typeof endpoint &
  typeof exportMessages &
  typeof exportDialog &
  typeof forwarding &
  typeof home &
  typeof kinds &
  typeof live &
  typeof rules &
  typeof sample &
  typeof script &
  typeof selection &
  typeof state &
  typeof workspace;

export type SourceKey = keyof SourceMessages;
