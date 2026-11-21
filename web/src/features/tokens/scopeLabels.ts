import type { TFunction } from 'i18next';

export function scopeLabel(t: TFunction, scope: string): string {
  switch (scope) {
    case 'read':
      return t('wl.tokens.scope.read');
    case 'endpoints:write':
      return t('wl.tokens.scope.endpointsWrite');
    case 'captures:write':
      return t('wl.tokens.scope.capturesWrite');
    case 'rules:write':
      return t('wl.tokens.scope.rulesWrite');
    case 'send':
      return t('wl.tokens.scope.send');
    default:
      return scope;
  }
}
