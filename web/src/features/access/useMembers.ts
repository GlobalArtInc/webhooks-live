import { useMutation, useQuery, useQueryClient, type QueryClient } from '@tanstack/react-query';
import { listMembers, putMember, removeMember } from '../../api/client';
import { shouldRetryRequest } from '../../api/queries';
import type { Member, MemberInput, MemberList } from '../../api/types';

export const membersKey = (endpointId: string) => ['members', endpointId] as const;

function updateMembers(queryClient: QueryClient, endpointId: string, update: (items: Member[]) => Member[]): void {
  queryClient.setQueryData<MemberList>(membersKey(endpointId), (list) =>
    list === undefined ? list : { items: update(list.items) },
  );
}

export function replaceMember(items: Member[], member: Member): Member[] {
  const exists = items.some((item) => item.user.id === member.user.id);
  return exists ? items.map((item) => (item.user.id === member.user.id ? member : item)) : [...items, member];
}

export function useMembers(endpointId: string, enabled: boolean) {
  return useQuery({
    queryKey: membersKey(endpointId),
    queryFn: () => listMembers(endpointId),
    enabled,
    retry: shouldRetryRequest,
  });
}

export function usePutMember(endpointId: string) {
  const queryClient = useQueryClient();
  return useMutation({
    mutationFn: (input: MemberInput) => putMember(endpointId, input),
    onSuccess: (member) => {
      updateMembers(queryClient, endpointId, (items) => replaceMember(items, member));
    },
  });
}

export function useRemoveMember(endpointId: string) {
  const queryClient = useQueryClient();
  return useMutation({
    mutationFn: (userId: string) => removeMember(endpointId, userId),
    onSuccess: (_result, userId) => {
      updateMembers(queryClient, endpointId, (items) => items.filter((item) => item.user.id !== userId));
    },
  });
}
