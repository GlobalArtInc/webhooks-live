export type UnreadTitleFormat = (title: string) => string;

let baseTitle: string | null = null;
let unreadFormat: UnreadTitleFormat | null = null;

function currentBase(): string {
  if (baseTitle === null) {
    baseTitle = document.title;
  }
  return baseTitle;
}

function apply(): void {
  const base = currentBase();
  document.title = unreadFormat === null ? base : unreadFormat(base);
}

export function setBaseTitle(title: string): void {
  baseTitle = title;
  apply();
}

export function setUnreadTitle(format: UnreadTitleFormat | null): void {
  unreadFormat = format;
  apply();
}
