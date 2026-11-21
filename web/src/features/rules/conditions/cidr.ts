const IPV4 = /^(\d{1,3})\.(\d{1,3})\.(\d{1,3})\.(\d{1,3})(?:\/(\d{1,2}))?$/;
const IPV6_CHARS = /^[0-9a-fA-F:]+$/;
const IPV6_GROUP = /^[0-9a-fA-F]{1,4}$/;
const MAX_IPV4_PREFIX = 32;
const MAX_IPV6_PREFIX = 128;
const MAX_OCTET = 255;
const MAX_IPV6_GROUPS = 8;

function isIpv4(text: string): boolean {
  const match = IPV4.exec(text);
  if (match === null) {
    return false;
  }
  const octets = match.slice(1, 5).map(Number);
  const prefix = match[5];
  return octets.every((octet) => octet <= MAX_OCTET) && (prefix === undefined || Number(prefix) <= MAX_IPV4_PREFIX);
}

function isIpv6(text: string): boolean {
  const [address = '', prefix, ...extra] = text.split('/');
  if (extra.length > 0 || !IPV6_CHARS.test(address)) {
    return false;
  }
  if (prefix !== undefined && !(/^\d{1,3}$/.test(prefix) && Number(prefix) <= MAX_IPV6_PREFIX)) {
    return false;
  }
  const halves = address.split('::');
  if (halves.length > 2) {
    return false;
  }
  const groups = halves.flatMap((half) => (half === '' ? [] : half.split(':')));
  if (!groups.every((group) => IPV6_GROUP.test(group))) {
    return false;
  }
  return halves.length === 2 ? groups.length < MAX_IPV6_GROUPS : groups.length === MAX_IPV6_GROUPS;
}

export function isCidr(text: string): boolean {
  return text.includes(':') ? isIpv6(text) : isIpv4(text);
}
