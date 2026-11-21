export const BADGE_MAX_DIGIT = 9;
export const FAVICON_SIZE_PX = 32;

const ICON_SELECTOR = 'link[rel~="icon"]';
const COLOR_TOKENS = {
  background: '--color-text',
  stream: '--color-surface',
  badge: '--color-accent',
  badgeText: '--color-text-on-accent',
} as const;

interface FaviconColors {
  background: string;
  stream: string;
  badge: string;
  badgeText: string;
}

let originalHref: string | null = null;
let originalType: string | null = null;
let createdLink: HTMLLinkElement | null = null;

export function badgeLabel(count: number): string {
  return count > BADGE_MAX_DIGIT ? `${BADGE_MAX_DIGIT}+` : String(count);
}

export type TitleFormat = (count: number, title: string) => string;

export function tabTitle(base: string, count: number, format: TitleFormat): string {
  return count > 0 ? format(count, base) : base;
}

function readColors(): FaviconColors | null {
  const style = getComputedStyle(document.documentElement);
  const read = (token: string): string => style.getPropertyValue(token).trim();
  const colors: FaviconColors = {
    background: read(COLOR_TOKENS.background),
    stream: read(COLOR_TOKENS.stream),
    badge: read(COLOR_TOKENS.badge),
    badgeText: read(COLOR_TOKENS.badgeText),
  };
  return Object.values(colors).some((color) => color === '') ? null : colors;
}

function drawStream(context: CanvasRenderingContext2D, colors: FaviconColors): void {
  const size = FAVICON_SIZE_PX;
  const margin = size / 16;
  context.fillStyle = colors.background;
  context.beginPath();
  context.roundRect(margin, margin, size - margin * 2, size - margin * 2, size / 5);
  context.fill();
  context.strokeStyle = colors.stream;
  context.lineWidth = size / 10;
  context.lineCap = 'round';
  for (const row of [0.34, 0.5, 0.66]) {
    context.beginPath();
    context.moveTo(size * 0.22, size * row);
    context.lineTo(size * (row === 0.5 ? 0.62 : 0.5), size * row);
    context.stroke();
  }
}

function drawBadge(context: CanvasRenderingContext2D, colors: FaviconColors, label: string): void {
  const size = FAVICON_SIZE_PX;
  const radius = size * 0.34;
  context.fillStyle = colors.badge;
  context.beginPath();
  context.arc(size - radius, size - radius, radius, 0, Math.PI * 2);
  context.fill();
  context.fillStyle = colors.badgeText;
  context.font = `700 ${label.length > 1 ? 13 : 17}px sans-serif`;
  context.textAlign = 'center';
  context.textBaseline = 'middle';
  context.fillText(label, size - radius, size - radius + 1);
}

export function renderFaviconBadge(count: number): string | null {
  const colors = readColors();
  if (colors === null) {
    return null;
  }
  const canvas = document.createElement('canvas');
  canvas.width = FAVICON_SIZE_PX;
  canvas.height = FAVICON_SIZE_PX;
  const context = canvas.getContext('2d');
  if (context === null) {
    return null;
  }
  drawStream(context, colors);
  drawBadge(context, colors, badgeLabel(count));
  return canvas.toDataURL('image/png');
}

function iconLink(): HTMLLinkElement {
  const existing = document.head.querySelector<HTMLLinkElement>(ICON_SELECTOR);
  if (existing !== null) {
    return existing;
  }
  const link = document.createElement('link');
  link.rel = 'icon';
  document.head.append(link);
  createdLink = link;
  return link;
}

export function showFaviconBadge(count: number): boolean {
  const dataUrl = renderFaviconBadge(count);
  if (dataUrl === null) {
    return false;
  }
  const link = iconLink();
  if (originalHref === null) {
    originalHref = link.getAttribute('href') ?? '';
    originalType = link.getAttribute('type');
  }
  link.type = 'image/png';
  link.href = dataUrl;
  return true;
}

export function restoreFavicon(): void {
  if (originalHref === null) {
    return;
  }
  const link = document.head.querySelector<HTMLLinkElement>(ICON_SELECTOR);
  if (link !== null) {
    if (link === createdLink) {
      link.remove();
      createdLink = null;
    } else {
      link.setAttribute('href', originalHref);
      if (originalType === null) {
        link.removeAttribute('type');
      } else {
        link.setAttribute('type', originalType);
      }
    }
  }
  originalHref = null;
  originalType = null;
}
