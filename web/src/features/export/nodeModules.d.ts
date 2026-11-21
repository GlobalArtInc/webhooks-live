declare module 'node:child_process' {
  export interface ExecFileOptions {
    cwd?: string;
    timeout?: number;
    env?: Record<string, string>;
  }

  export interface SpawnSyncResult {
    status: number | null;
    stdout: Uint8Array;
  }

  export function execFile(
    file: string,
    args: readonly string[],
    options: ExecFileOptions,
    callback: (error: Error | null) => void,
  ): unknown;

  export function spawnSync(file: string, args: readonly string[]): SpawnSyncResult;
}

declare module 'node:fs' {
  export interface RmOptions {
    recursive?: boolean;
    force?: boolean;
  }

  export function existsSync(path: string): boolean;
  export function writeFileSync(path: string, data: Uint8Array): void;
  export function mkdtempSync(prefix: string): string;
  export function rmSync(path: string, options?: RmOptions): void;
}

declare module 'node:http' {
  export interface ListeningAddress {
    port: number;
  }

  export interface IncomingMessage extends AsyncIterable<Uint8Array> {
    method?: string;
    url?: string;
    rawHeaders: string[];
  }

  export interface ServerResponse {
    statusCode: number;
    end(): void;
  }

  export interface Server {
    listen(port: number, host: string, callback: () => void): Server;
    close(callback: () => void): Server;
    address(): ListeningAddress | string | null;
  }

  export function createServer(listener: (req: IncomingMessage, res: ServerResponse) => void): Server;
}

declare module 'node:os' {
  export function tmpdir(): string;
}

declare module 'node:path' {
  export function join(...parts: string[]): string;
}

declare module 'node:process' {
  export interface ProcessInfo {
    platform: string;
    env: Record<string, string | undefined>;
  }

  const processInfo: ProcessInfo;
  export default processInfo;
}
