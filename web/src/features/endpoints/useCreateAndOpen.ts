import { useNavigate } from '@tanstack/react-router';
import { useCallback } from 'react';
import { useCreateEndpoint } from '../../api/queries';
import { useRailState } from '../../app/shell/railState';
import type { EndpointInput } from '../../api/types';
import { rememberEndpoint } from './store';

export interface CreateAndOpen {
  create: (input: EndpointInput) => void;
  isPending: boolean;
  error: Error | null;
}

function focusMain(): void {
  document.getElementById('main')?.focus();
}

export function useCreateAndOpen(): CreateAndOpen {
  const navigate = useNavigate();
  const mutation = useCreateEndpoint();
  const { closeDrawer } = useRailState();
  const { mutate, isPending, error } = mutation;

  const create = useCallback(
    (input: EndpointInput) => {
      mutate(input, {
        onSuccess: (endpoint) => {
          rememberEndpoint(endpoint.id, endpoint.title);
          closeDrawer();
          void navigate({ to: '/e/$id', params: { id: endpoint.id } }).then(focusMain);
        },
      });
    },
    [mutate, navigate, closeDrawer],
  );

  return { create, isPending, error };
}
