import { useSharedNow } from '../../components/format/useSharedNow';

export function useNow(freshest: number | null = null): number {
  return useSharedNow(freshest);
}
