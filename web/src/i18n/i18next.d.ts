import 'i18next';
import type { SourceMessages } from './keys';

interface SourceResources {
  translation: SourceMessages;
}

declare module 'i18next' {
  interface CustomTypeOptions {
    defaultNS: 'translation';
    keySeparator: false;
    resources: SourceResources;
  }
}
