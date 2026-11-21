export interface FollowState {
  enabled: boolean;
  paused: boolean;
}

export type FollowAction =
  { kind: 'toggle' } | { kind: 'resume' } | { kind: 'select'; captureId: string; newestId: string | null };

export function isFollowing(state: FollowState): boolean {
  return state.enabled && !state.paused;
}

export function reduceFollow(state: FollowState, action: FollowAction): FollowState {
  switch (action.kind) {
    case 'toggle':
      if (state.enabled && state.paused) {
        return { enabled: true, paused: false };
      }
      return { enabled: !state.enabled, paused: false };
    case 'resume':
      return state.enabled && state.paused ? { enabled: true, paused: false } : state;
    case 'select':
      return isFollowing(state) && action.captureId !== action.newestId ? { enabled: true, paused: true } : state;
  }
}
