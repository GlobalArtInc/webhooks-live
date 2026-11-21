import type { ConditionLimits, TriggerKind } from './types';

export const TRIGGER_KINDS: readonly TriggerKind[] = ['http', 'email', 'dns'];

export const DEFAULT_LIMITS: ConditionLimits = { maxDepth: 4, maxLeaves: 32 };

export const VARIABLE_SUBJECT_ID = 'variable';

export const CONFLICT_STATUSES: readonly number[] = [409, 412];
