import type { TFunction } from 'i18next';
import type { WorkspaceLabelKey, WorkspaceSectionId } from '../features/workspace/sections';
import { isEndpointId } from './ids';

const ID_HEAD = 3;
const ID_TAIL = 2;
const ELLIPSIS = '…';
const ENDPOINT_PATH = /^\/e\/([^/]+)(?:\/(.*))?$/;
const SETTINGS_PATH = /^settings(?:\/([^/]+))?(?:\/.*)?$/;
const CAPTURE_PATH = /^c\/[^/]+$/;
const DIFF_PATH = /^diff\/[^/]+\/[^/]+$/;

const SECTION_LABELS: Readonly<Record<WorkspaceSectionId, WorkspaceLabelKey>> = {
  general: 'wl.workspace.section.general',
  response: 'wl.workspace.section.response',
  rules: 'wl.workspace.section.rules',
  forwarding: 'wl.workspace.section.forwarding',
  access: 'wl.workspace.section.access',
  notifications: 'wl.workspace.section.notifications',
  schedules: 'wl.workspace.section.schedules',
  secrets: 'wl.workspace.section.secrets',
  cli: 'wl.workspace.section.cli',
};

function isSectionId(value: string): value is WorkspaceSectionId {
  return Object.hasOwn(SECTION_LABELS, value);
}

export function shortTitleId(id: string): string {
  return `${id.slice(0, ID_HEAD)}${ELLIPSIS}${id.slice(-ID_TAIL)}`;
}

export function endpointIdOfPath(pathname: string): string | null {
  const match = ENDPOINT_PATH.exec(pathname.length > 1 ? pathname.replace(/\/+$/, '') : pathname);
  const id = match?.[1];
  return id !== undefined && isEndpointId(id) ? id : null;
}

function endpointName(t: TFunction, id: string, endpointTitle: string | null): string {
  return endpointTitle ?? t('wl.title.endpoint', { id: shortTitleId(id) });
}

function endpointPage(t: TFunction, id: string, rest: string, endpointTitle: string | null): string {
  if (!isEndpointId(id)) {
    return t('wl.title.notFound');
  }
  const endpoint = endpointName(t, id, endpointTitle);
  if (rest === '' || CAPTURE_PATH.test(rest)) {
    return endpoint;
  }
  if (rest === 'unlock') {
    return t('wl.title.unlock');
  }
  if (rest === 'forwarding') {
    return t('wl.title.settings', { endpoint, section: t('wl.workspace.section.forwarding') });
  }
  if (DIFF_PATH.test(rest)) {
    return t('wl.title.diff');
  }
  const settings = SETTINGS_PATH.exec(rest);
  if (settings === null) {
    return t('wl.title.notFound');
  }
  const section = settings[1];
  if (section === undefined) {
    return t('wl.title.settingsRoot', { endpoint });
  }
  return isSectionId(section)
    ? t('wl.title.settings', { endpoint, section: t(SECTION_LABELS[section]) })
    : t('wl.title.notFound');
}

function pageName(t: TFunction, pathname: string, endpointTitle: string | null): string | null {
  const path = pathname.length > 1 ? pathname.replace(/\/+$/, '') : pathname;
  if (path === '/') {
    return null;
  }
  const endpoint = ENDPOINT_PATH.exec(path);
  if (endpoint !== null) {
    return endpointPage(t, endpoint[1] ?? '', endpoint[2] ?? '', endpointTitle);
  }
  switch (path) {
    case '/endpoints':
      return t('wl.title.endpoints');
    case '/send':
      return t('wl.title.composer');
    case '/inbox':
      return t('wl.title.inbox');
    case '/usage':
      return t('wl.title.usage');
    case '/account':
      return t('wl.title.account');
    case '/settings':
      return t('wl.title.preferences');
    case '/settings/tokens':
      return t('wl.title.tokens');
    case '/signin':
      return t('wl.title.signin');
    default:
      return path === '/docs/api' || path.startsWith('/docs/api/')
        ? t('wl.title.apiReference')
        : t('wl.title.notFound');
  }
}

export function pageTitleOf(pathname: string, t: TFunction, endpointTitle: string | null = null): string {
  const site = t('wl.title.site');
  const page = pageName(t, pathname, endpointTitle);
  return page === null ? site : t('wl.title.page', { page, site });
}
