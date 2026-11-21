export const CLI_REPOSITORY_URL = 'https://github.com/GlobalArtInc/webhooks-live';
export const CLI_RELEASE_VERSION = '0.1.0-dev';
export const CLI_BINARY = 'webhooks-live-cli';
export const CLI_CRATE = 'whl-cli';
export const CLI_INSTALL_DIR = '/usr/local/bin';

export type CliPlatformId = 'linuxAmd64' | 'linuxArm64' | 'macosArm64' | 'macosAmd64';

export interface CliPlatform {
  id: CliPlatformId;
  asset: string;
  checksum: string;
}

export const CLI_PLATFORMS: readonly CliPlatform[] = [
  { id: 'linuxAmd64', asset: 'linux-amd64', checksum: 'sha256sum -c -' },
  { id: 'linuxArm64', asset: 'linux-arm64', checksum: 'sha256sum -c -' },
  { id: 'macosArm64', asset: 'macos-arm64', checksum: 'shasum -a 256 -c -' },
  { id: 'macosAmd64', asset: 'macos-amd64', checksum: 'shasum -a 256 -c -' },
];

export function releaseTag(): string {
  return `cli-v${CLI_RELEASE_VERSION}`;
}

export function archiveName(platform: CliPlatform): string {
  return `${CLI_BINARY}-${CLI_RELEASE_VERSION}-${platform.asset}.tar.gz`;
}

export function unpackedDirName(platform: CliPlatform): string {
  return `${CLI_BINARY}-${CLI_RELEASE_VERSION}-${platform.asset}`;
}

export function checksumsName(): string {
  return `${CLI_BINARY}-${CLI_RELEASE_VERSION}-checksums.txt`;
}

export function releaseAssetUrl(name: string): string {
  return `${CLI_REPOSITORY_URL}/releases/download/${releaseTag()}/${name}`;
}

export function verifyCommand(platform: CliPlatform): string {
  const checksums = checksumsName();
  return `grep " ${archiveName(platform)}$" ${checksums} | ${platform.checksum}`;
}

export function installCommand(platform: CliPlatform): string {
  const archive = archiveName(platform);
  const binary = `${unpackedDirName(platform)}/${CLI_BINARY}`;
  return [`tar -xzf ${archive}`, `sudo install -m 0755 ${binary} ${CLI_INSTALL_DIR}/${CLI_BINARY}`].join('\n');
}

export function sourceInstallCommand(): string {
  return `cargo install --locked --git ${CLI_REPOSITORY_URL} ${CLI_CRATE}`;
}
