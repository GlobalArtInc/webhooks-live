export function formatDate(iso: string, locale: string): string {
  const time = Date.parse(iso);
  if (Number.isNaN(time)) {
    return iso;
  }
  return new Intl.DateTimeFormat(locale, { dateStyle: 'medium' }).format(time);
}
