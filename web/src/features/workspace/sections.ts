import { lazy, type ComponentType, type LazyExoticComponent } from 'react';
import type { Endpoint } from '../../api/types';

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

export const WORKSPACE_SECTIONS: readonly WorkspaceSection[] = [
  { id: 'general', labelKey: 'wl.workspace.section.general', Component: GeneralSection },
  { id: 'response', labelKey: 'wl.workspace.section.response', Component: ResponseSection },
  { id: 'rules', labelKey: 'wl.workspace.section.rules', Component: null },
  {
    id: 'forwarding',
    labelKey: 'wl.workspace.section.forwarding',
    Component: ForwardingSection,
    count: (endpoint) => endpoint.forward_rules.length,
  },
  { id: 'access', labelKey: 'wl.workspace.section.access', Component: null },
  { id: 'notifications', labelKey: 'wl.workspace.section.notifications', Component: null },
  { id: 'schedules', labelKey: 'wl.workspace.section.schedules', Component: null },
  { id: 'cli', labelKey: 'wl.workspace.section.cli', Component: null },
];

export function isWorkspaceSectionId(value: string): value is WorkspaceSectionId {
  return (WORKSPACE_SECTION_IDS as readonly string[]).includes(value);
}

export function findWorkspaceSection(id: string): WorkspaceSection | undefined {
  return WORKSPACE_SECTIONS.find((section) => section.id === id);
}
