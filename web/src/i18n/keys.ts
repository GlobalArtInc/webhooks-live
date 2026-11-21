import access from './en/access.json';
import common from './en/common.json';
import composer from './en/composer.json';
import conditions from './en/conditions.json';
import detail from './en/detail.json';
import diff from './en/diff.json';
import editor from './en/editor.json';
import email from './en/email.json';
import endpoint from './en/endpoint.json';
import exportMessages from './en/export.json';
import exportDialog from './en/exportdialog.json';
import forwarding from './en/forwarding.json';
import home from './en/home.json';
import kinds from './en/kinds.json';
import inbox from './en/inbox.json';
import live from './en/live.json';
import rules from './en/rules.json';
import rail from './en/rail.json';
import sample from './en/sample.json';
import script from './en/script.json';
import selection from './en/selection.json';
import schedules from './en/schedules.json';
import state from './en/state.json';
import steps from './en/steps.json';
import tokens from './en/tokens.json';
import usage from './en/usage.json';
import templating from './en/templating.json';
import workspace from './en/workspace.json';

export type SourceMessages = typeof access &
  typeof common &
  typeof composer &
  typeof conditions &
  typeof detail &
  typeof diff &
  typeof editor &
  typeof email &
  typeof endpoint &
  typeof exportMessages &
  typeof exportDialog &
  typeof forwarding &
  typeof home &
  typeof kinds &
  typeof inbox &
  typeof live &
  typeof rules &
  typeof rail &
  typeof sample &
  typeof script &
  typeof selection &
  typeof schedules &
  typeof state &
  typeof steps &
  typeof tokens &
  typeof usage &
  typeof templating &
  typeof workspace;

export type SourceKey = keyof SourceMessages;
