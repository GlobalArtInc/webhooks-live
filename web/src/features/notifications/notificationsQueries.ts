import { useMutation, useQuery, useQueryClient } from '@tanstack/react-query';
import { shouldRetryRequest } from '../../api/queries';
import {
  addEmailAddress,
  createChannel,
  deleteChannel,
  deleteEmailAddress,
  listChannels,
  listEmailAddresses,
  listSecrets,
  patchChannel,
  testChannel,
} from './notificationsApi';
import type { ChannelInput } from './types';

export interface ChannelUpdate {
  channelId: string;
  input: ChannelInput;
}

const NOTIFICATIONS_KEY = 'notifications';

export const notificationKeys = {
  channels: (endpointId: string) => [NOTIFICATIONS_KEY, endpointId] as const,
  secrets: (endpointId: string) => [NOTIFICATIONS_KEY, endpointId, 'secrets'] as const,
  emailAddresses: () => [NOTIFICATIONS_KEY, 'email-addresses'] as const,
};

export function useChannels(endpointId: string, enabled: boolean) {
  return useQuery({
    queryKey: notificationKeys.channels(endpointId),
    queryFn: () => listChannels(endpointId),
    retry: shouldRetryRequest,
    enabled,
  });
}

export function useSecrets(endpointId: string, enabled: boolean) {
  return useQuery({
    queryKey: notificationKeys.secrets(endpointId),
    queryFn: () => listSecrets(endpointId),
    retry: shouldRetryRequest,
    enabled,
  });
}

export function useEmailAddresses(enabled: boolean) {
  return useQuery({
    queryKey: notificationKeys.emailAddresses(),
    queryFn: listEmailAddresses,
    retry: shouldRetryRequest,
    enabled,
  });
}

export function useCreateChannel(endpointId: string) {
  const queryClient = useQueryClient();
  return useMutation({
    mutationFn: (input: ChannelInput) => createChannel(endpointId, input),
    onSuccess: () => queryClient.invalidateQueries({ queryKey: notificationKeys.channels(endpointId) }),
  });
}

export function useUpdateChannel(endpointId: string) {
  const queryClient = useQueryClient();
  return useMutation({
    mutationFn: (variables: ChannelUpdate) => patchChannel(endpointId, variables.channelId, variables.input),
    onSuccess: () => queryClient.invalidateQueries({ queryKey: notificationKeys.channels(endpointId) }),
  });
}

export function useDeleteChannel(endpointId: string) {
  const queryClient = useQueryClient();
  return useMutation({
    mutationFn: (channelId: string) => deleteChannel(endpointId, channelId),
    onSuccess: () => queryClient.invalidateQueries({ queryKey: notificationKeys.channels(endpointId) }),
  });
}

export function useTestChannel(endpointId: string) {
  return useMutation({
    mutationFn: (channelId: string) => testChannel(endpointId, channelId),
  });
}

export function useAddEmailAddress() {
  const queryClient = useQueryClient();
  return useMutation({
    mutationFn: (address: string) => addEmailAddress(address),
    onSuccess: () => queryClient.invalidateQueries({ queryKey: notificationKeys.emailAddresses() }),
  });
}

export function useDeleteEmailAddress() {
  const queryClient = useQueryClient();
  return useMutation({
    mutationFn: (addressId: string) => deleteEmailAddress(addressId),
    onSuccess: () => queryClient.invalidateQueries({ queryKey: notificationKeys.emailAddresses() }),
  });
}
