import type { Page } from '@playwright/test';

export interface DisabledContrast {
  name: string;
  ratio: number;
}

export async function disabledButtonContrasts(page: Page): Promise<DisabledContrast[]> {
  return page.evaluate(() => {
    type Rgba = [number, number, number, number];

    function parse(value: string): Rgba {
      const match = /rgba?\(([^)]+)\)/.exec(value);
      const parts = (match?.[1] ?? '0,0,0,0').split(/[,\s/]+/).filter((part) => part !== '');
      const [red = 0, green = 0, blue = 0, alpha = 1] = parts.map(Number);
      return [red, green, blue, alpha];
    }

    function over(top: Rgba, bottom: Rgba): Rgba {
      const alpha = top[3] + bottom[3] * (1 - top[3]);
      if (alpha === 0) {
        return [0, 0, 0, 0];
      }
      const mix = (index: 0 | 1 | 2) => (top[index] * top[3] + bottom[index] * bottom[3] * (1 - top[3])) / alpha;
      return [mix(0), mix(1), mix(2), alpha];
    }

    function luminance(color: Rgba): number {
      const [red, green, blue] = color.slice(0, 3).map((channel) => {
        const unit = channel / 255;
        return unit <= 0.03928 ? unit / 12.92 : ((unit + 0.055) / 1.055) ** 2.4;
      });
      return 0.2126 * (red ?? 0) + 0.7152 * (green ?? 0) + 0.0722 * (blue ?? 0);
    }

    function backdrop(element: Element): Rgba {
      let color: Rgba = [0, 0, 0, 0];
      for (let node: Element | null = element; node !== null; node = node.parentElement) {
        color = over(color, parse(getComputedStyle(node).backgroundColor));
        if (color[3] >= 1) {
          return color;
        }
      }
      return over(color, [255, 255, 255, 1]);
    }

    function opacityOf(element: Element): number {
      let opacity = 1;
      for (let node: Element | null = element; node !== null; node = node.parentElement) {
        opacity *= Number(getComputedStyle(node).opacity);
      }
      return opacity;
    }

    const results: { name: string; ratio: number }[] = [];
    for (const button of document.querySelectorAll('button:disabled')) {
      const box = button.getBoundingClientRect();
      if (box.width === 0 || box.height === 0) {
        continue;
      }
      const fillRaw = parse(getComputedStyle(button).backgroundColor);
      const fill =
        fillRaw[3] === 0
          ? backdrop(button.parentElement ?? button)
          : over(fillRaw, backdrop(button.parentElement ?? button));
      const opacity = opacityOf(button);
      const page = backdrop(button.parentElement ?? button);
      const text = over(parse(getComputedStyle(button).color), fill);
      const shownFill = over([fill[0], fill[1], fill[2], opacity], page);
      const shownText = over([text[0], text[1], text[2], opacity], page);
      const lighter = Math.max(luminance(shownText), luminance(shownFill));
      const darker = Math.min(luminance(shownText), luminance(shownFill));
      results.push({
        name: (button.getAttribute('aria-label') ?? button.textContent ?? '').trim(),
        ratio: (lighter + 0.05) / (darker + 0.05),
      });
    }
    return results;
  });
}
