export type EmailSectionId = 'summary' | 'text' | 'attachments' | 'headers' | 'raw';

export const INITIALLY_COLLAPSED: readonly EmailSectionId[] = ['raw'];
