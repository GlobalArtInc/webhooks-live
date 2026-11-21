import { useNavigate } from '@tanstack/react-router';
import { useCallback } from 'react';
import { rememberedSelection } from './selectionMemory';

export function useCloseSheet(endpointId: string): () => void {
  const navigate = useNavigate();
  return useCallback(() => {
    const captureId = rememberedSelection(endpointId);
    if (captureId !== null) {
      void navigate({ to: '/e/$id/c/$captureId', params: { id: endpointId, captureId }, search: true });
      return;
    }
    void navigate({ to: '/e/$id', params: { id: endpointId }, search: true });
  }, [endpointId, navigate]);
}
