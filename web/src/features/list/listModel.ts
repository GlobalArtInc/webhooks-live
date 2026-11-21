import type { CaptureListData } from '../../api/queries';
import type { CapturePage, CaptureSummary } from '../../api/types';

export const NOT_FOUND = -1;

export function flattenCaptures(data: CaptureListData | undefined): CaptureSummary[] {
  if (data === undefined) {
    return [];
  }
  const seen = new Set<string>();
  return data.pages.flatMap((page) =>
    page.items.filter((item) => {
      if (seen.has(item.id)) {
        return false;
      }
      seen.add(item.id);
      return true;
    }),
  );
}

export function totalOf(data: CaptureListData | undefined): number | null {
  const first = data?.pages[0];
  return first === undefined ? null : first.total;
}

export function indexOfCapture(items: readonly CaptureSummary[], captureId: string): number {
  return items.findIndex((item) => item.id === captureId);
}

export function neighbourOf(items: readonly CaptureSummary[], captureId: string): string | null {
  const index = indexOfCapture(items, captureId);
  if (index === NOT_FOUND) {
    return null;
  }
  return items[index + 1]?.id ?? items[index - 1]?.id ?? null;
}

interface InsertionPoint {
  page: number;
  index: number;
}

function findInsertionPoint(pages: readonly CapturePage[], seq: number): InsertionPoint | null {
  for (const [page, value] of pages.entries()) {
    const index = value.items.findIndex((item) => item.seq < seq);
    if (index !== NOT_FOUND) {
      return { page, index };
    }
  }
  return null;
}

export function restoreCaptureInPages(data: CaptureListData, capture: CaptureSummary): CaptureListData {
  const cached = data.pages.some((page) => page.items.some((item) => item.id === capture.id));
  const last = data.pages.length - 1;
  if (cached || last < 0) {
    return data;
  }
  const point = findInsertionPoint(data.pages, capture.seq) ?? {
    page: last,
    index: data.pages[last]?.items.length ?? 0,
  };
  return {
    ...data,
    pages: data.pages.map((page, pageIndex) => {
      if (pageIndex !== point.page) {
        return { ...page, total: page.total + 1 };
      }
      const items = [...page.items];
      items.splice(point.index, 0, capture);
      return { ...page, items, total: page.total + 1 };
    }),
  };
}
