import type { InfiniteData, QueryKey } from '@tanstack/react-query';
import type { CaptureSummary, Page } from '../../api/types';

export type InboxListData = InfiniteData<Page<CaptureSummary>, string | null>;

export function inboxListKey(scopeKey: string): QueryKey {
  return ['inbox', scopeKey, 'captures'];
}

export function mergeInboxCapture(data: InboxListData, capture: CaptureSummary): InboxListData {
  const known = data.pages.some((page) => page.items.some((item) => item.id === capture.id));
  if (known) {
    return {
      ...data,
      pages: data.pages.map((page) => ({
        ...page,
        items: page.items.map((item) => (item.id === capture.id ? capture : item)),
      })),
    };
  }
  const [first, ...rest] = data.pages;
  if (first === undefined) {
    return data;
  }
  return { ...data, pages: [{ ...first, items: [capture, ...first.items] }, ...rest] };
}

export function removeInboxCapture(data: InboxListData, captureId: string): InboxListData {
  const known = data.pages.some((page) => page.items.some((item) => item.id === captureId));
  if (!known) {
    return data;
  }
  return {
    ...data,
    pages: data.pages.map((page) => ({
      ...page,
      items: page.items.filter((item) => item.id !== captureId),
    })),
  };
}
