import type { EventSourceFactory, EventSourceLike } from './LiveSource';

const CONNECTING = 0;
const OPEN = 1;
const CLOSED = 2;

type Listener = (event: Event) => void;

export class FakeEventSource implements EventSourceLike {
  readyState = CONNECTING;
  readonly url: string;
  private readonly listeners = new Map<string, Listener[]>();

  constructor(url: string) {
    this.url = url;
  }

  addEventListener(type: string, listener: Listener): void {
    this.listeners.set(type, [...(this.listeners.get(type) ?? []), listener]);
  }

  close(): void {
    this.readyState = CLOSED;
  }

  isClosed(): boolean {
    return this.readyState === CLOSED;
  }

  open(): void {
    this.readyState = OPEN;
    this.dispatch('open', new Event('open'));
  }

  send(name: string, data: unknown, lastEventId = ''): void {
    this.dispatch(name, new MessageEvent(name, { data: JSON.stringify(data), lastEventId }));
  }

  sendRaw(name: string, data: string): void {
    this.dispatch(name, new MessageEvent(name, { data }));
  }

  fail(closedByBrowser: boolean): void {
    this.readyState = closedByBrowser ? CLOSED : CONNECTING;
    this.dispatch('error', new Event('error'));
  }

  private dispatch(type: string, event: Event): void {
    for (const listener of this.listeners.get(type) ?? []) {
      listener(event);
    }
  }
}

export interface FakeEventSources {
  factory: EventSourceFactory;
  instances: FakeEventSource[];
  latest: () => FakeEventSource;
}

export function createFakeEventSources(): FakeEventSources {
  const instances: FakeEventSource[] = [];
  return {
    instances,
    factory: (url) => {
      const source = new FakeEventSource(url);
      instances.push(source);
      return source;
    },
    latest: () => {
      const source = instances.at(-1);
      if (source === undefined) {
        throw new Error('no event source has been created');
      }
      return source;
    },
  };
}
