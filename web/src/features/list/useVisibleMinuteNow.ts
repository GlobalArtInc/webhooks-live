import { useSharedNow } from '../../components/format/useSharedNow';

export function useVisibleMinuteNow(freshest: number | null = null): number {
  return useSharedNow(freshest);
}
