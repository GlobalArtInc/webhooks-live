import { lazy, type ComponentType, type LazyExoticComponent } from 'react';
import type { Endpoint, Instance } from '../../api/types';

export const WORKSPACE_SECTION_IDS = [
  'general',
  'response',
  'rules',
  'forwarding',
  'access',
  'notifications',
  'schedules',
  'cli',
] as const;

export type WorkspaceSectionId = (typeof WORKSPACE_SECTION_IDS)[number];

export type WorkspaceLabelKey =
  | 'wl.workspace.section.general'
  | 'wl.workspace.section.response'
  | 'wl.workspace.section.rules'
  | 'wl.workspace.section.forwarding'
  | 'wl.workspace.section.access'
  | 'wl.workspace.section.notifications'
  | 'wl.workspace.section.schedules'
  | 'wl.workspace.section.cli';

export interface SectionProps {
  endpointId: string;
}

export interface WorkspaceSection {
  id: WorkspaceSectionId;
  labelKey: WorkspaceLabelKey;
  Component: LazyExoticComponent<ComponentType<SectionProps>> | null;
  count?: (endpoint: Endpoint) => number;
  available?: (instance: Instance | undefined) => boolean;
}

const GeneralSection = lazy(() =>
  import('./general/GeneralSection').then((module) => ({ default: module.GeneralSection })),
);

const ResponseSection = lazy(() =>
  import('./response/ResponseSection').then((module) => ({ default: module.ResponseSection })),
);

const ForwardingSection = lazy(() =>
  import('../forwarding/ForwardingSection').then((module) => ({ default: module.ForwardingSection })),
);

const RulesSection = lazy(() => import('../rules/RulesSection').then((module) => ({ default: module.RulesSection })));
const AccessSection = lazy(() =>
  import('../access/AccessSection').then((module) => ({ default: module.AccessSection })),
);
const ScheduleSection = lazy(() =>
  import('../schedules/ScheduleSection').then((module) => ({ default: module.ScheduleSection })),
);
const CliSection = lazy(() => import('../cli/CliSection').then((module) => ({ default: module.CliSection })));
const NotificationsSection = lazy(() =>
  import('../notifications/NotificationsSection').then((module) => ({ default: module.NotificationsSection })),
);

export const WORKSPACE_SECTIONS: readonly WorkspaceSection[] = [
  { id: 'general', labelKey: 'wl.workspace.section.general', Component: GeneralSection },
  { id: 'response', labelKey: 'wl.workspace.section.response', Component: ResponseSection },
  { id: 'rules', labelKey: 'wl.workspace.section.rules', Component: RulesSection },
  {
    id: 'forwarding',
    labelKey: 'wl.workspace.section.forwarding',
    Component: ForwardingSection,
    count: (endpoint) => endpoint.forward_rules.length,
  },
  { id: 'access', labelKey: 'wl.workspace.section.access', Component: AccessSection },
  {
    id: 'notifications',
    labelKey: 'wl.workspace.section.notifications',
    Component: NotificationsSection,
    available: (instance) => (instance?.features.notifications ?? []).length > 0,
  },
  {
    id: 'schedules',
    labelKey: 'wl.workspace.section.schedules',
    Component: ScheduleSection,
    available: (instance) => instance?.features.schedules === true,
  },
  { id: 'cli', labelKey: 'wl.workspace.section.cli', Component: CliSection },
];

export function isWorkspaceSectionId(value: string): value is WorkspaceSectionId {
  return (WORKSPACE_SECTION_IDS as readonly string[]).includes(value);
}

export function findWorkspaceSection(id: string): WorkspaceSection | undefined {
  return WORKSPACE_SECTIONS.find((section) => section.id === id);
}
