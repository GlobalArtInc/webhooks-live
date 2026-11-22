import type {
  CaptureDeletedPayload,
  CaptureSummary,
  EmptyPayload,
  EndpointView,
  ForwardResultPayload,
  ResyncPayload,
  StreamReadyPayload,
} from '../../api/types';
import type { RuleResultPayload } from '../../api/ruleRunTypes';

export type ConnectionState = 'connecting' | 'open' | 'reconnecting' | 'offline' | 'ended';

export type EndReason = 'deleted' | 'gone' | 'missing' | 'locked';

export interface LiveReadyPayload extends StreamReadyPayload {
  capture_count: number;
}

export interface LivePayloads {
  ready: LiveReadyPayload;
  capture: CaptureSummary;
  capture_deleted: CaptureDeletedPayload;
  captures_cleared: EmptyPayload;
  endpoint_updated: EndpointView;
  endpoint_deleted: EmptyPayload;
  forward_result: ForwardResultPayload;
  rule_result: RuleResultPayload;
  resync: ResyncPayload;
  bye: EmptyPayload;
}

export type LiveEventName = keyof LivePayloads;

export interface LiveEventOf<K extends LiveEventName> {
  name: K;
  data: LivePayloads[K];
}

export type LiveEvent = { [K in LiveEventName]: LiveEventOf<K> }[LiveEventName];

export interface ConnectionChange {
  state: ConnectionState;
  reason: EndReason | null;
}

export interface InvalidEvent {
  name: LiveEventName;
}

export interface LiveSourceEvents extends LivePayloads {
  state: ConnectionChange;
  invalid: InvalidEvent;
}

export type LiveSourceEventName = keyof LiveSourceEvents;

export type LiveHandler<K extends LiveSourceEventName> = (payload: LiveSourceEvents[K]) => void;

export const LIVE_EVENT_NAMES: readonly LiveEventName[] = [
  'ready',
  'capture',
  'capture_deleted',
  'captures_cleared',
  'endpoint_updated',
  'endpoint_deleted',
  'forward_result',
  'rule_result',
  'resync',
  'bye',
];
