import type { Page } from '@playwright/test';

interface PageStream {
  url: string;
  closed: boolean;
  readyState: number;
  emit: (type: string, data: string | null, lastEventId: string) => void;
}

interface StreamWindow {
  __wlStreams: PageStream[];
}

interface PushArgs {
  name: string;
  payload: string;
  id: string;
}

export async function installFakeStream(page: Page): Promise<void> {
  await page.addInitScript(() => {
    const streams: unknown[] = [];

    class FakeEventSource {
      url: string;
      readyState = 0;
      closed = false;
      listeners: Record<string, ((event: Event) => void)[]> = {};

      constructor(url: string) {
        this.url = url;
        streams.push(this);
      }

      addEventListener(type: string, listener: (event: Event) => void): void {
        this.listeners[type] = [...(this.listeners[type] ?? []), listener];
      }

      close(): void {
        this.readyState = 2;
        this.closed = true;
      }

      emit(type: string, data: string | null, lastEventId: string): void {
        const event = data === null ? new Event(type) : new MessageEvent(type, { data, lastEventId });
        for (const listener of this.listeners[type] ?? []) {
          listener(event);
        }
      }
    }

    Object.defineProperty(window, '__wlStreams', { value: streams });
    Object.defineProperty(window, 'EventSource', { value: FakeEventSource, configurable: true, writable: true });
  });
}

export async function streamCount(page: Page): Promise<number> {
  return page.evaluate(() => (window as unknown as StreamWindow).__wlStreams.length);
}

export async function latestStreamUrl(page: Page): Promise<string> {
  return page.evaluate(() => (window as unknown as StreamWindow).__wlStreams.at(-1)?.url ?? '');
}

export async function isLatestStreamClosed(page: Page): Promise<boolean> {
  return page.evaluate(() => (window as unknown as StreamWindow).__wlStreams.at(-1)?.closed ?? true);
}

export async function openLatestStream(page: Page): Promise<void> {
  await page.evaluate(() => {
    const stream = (window as unknown as StreamWindow).__wlStreams.at(-1);
    if (stream === undefined) {
      throw new Error('the page opened no stream');
    }
    stream.readyState = 1;
    stream.emit('open', null, '');
  });
}

export async function pushStreamEvent(page: Page, name: string, data: unknown, id = ''): Promise<void> {
  const args: PushArgs = { name, payload: JSON.stringify(data), id };
  await page.evaluate((input) => {
    const stream = (window as unknown as StreamWindow).__wlStreams.at(-1);
    if (stream === undefined || stream.closed) {
      throw new Error('there is no open stream to push into');
    }
    stream.emit(input.name, input.payload, input.id);
  }, args);
}

export async function failLatestStream(page: Page): Promise<void> {
  await page.evaluate(() => {
    const stream = (window as unknown as StreamWindow).__wlStreams.at(-1);
    if (stream === undefined) {
      throw new Error('the page opened no stream');
    }
    stream.readyState = 0;
    stream.emit('error', null, '');
  });
}

export async function setTabHidden(page: Page, hidden: boolean): Promise<void> {
  await page.evaluate((value) => {
    Object.defineProperty(document, 'hidden', { configurable: true, get: () => value });
    Object.defineProperty(document, 'visibilityState', {
      configurable: true,
      get: () => (value ? 'hidden' : 'visible'),
    });
    document.dispatchEvent(new Event('visibilitychange'));
  }, hidden);
}
