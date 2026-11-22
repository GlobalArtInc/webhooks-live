import { expect, test, type Page } from '@playwright/test';
import { NARROW, SCREENS, WIDE } from './support/screens';

const MIN_FONT_PX = 14;

interface SmallText {
  tag: string;
  text: string;
  size: number;
}

async function smallTexts(page: Page, min: number): Promise<SmallText[]> {
  return page.evaluate((limit) => {
    function hiddenFromView(element: Element): boolean {
      for (let node: Element | null = element; node !== null; node = node.parentElement) {
        if (node.getAttribute('aria-hidden') === 'true') {
          return true;
        }
        const style = getComputedStyle(node);
        if (style.display === 'none' || style.visibility === 'hidden') {
          return true;
        }
      }
      const box = element.getBoundingClientRect();
      return box.width <= 1 || box.height <= 1;
    }

    const found: SmallText[] = [];
    const walker = document.createTreeWalker(document.body, NodeFilter.SHOW_TEXT);
    for (let node = walker.nextNode(); node !== null; node = walker.nextNode()) {
      const element = node.parentElement;
      const text = (node.textContent ?? '').trim();
      if (element === null || text === '' || ['SCRIPT', 'STYLE', 'OPTION'].includes(element.tagName)) {
        continue;
      }
      if (hiddenFromView(element)) {
        continue;
      }
      const size = parseFloat(getComputedStyle(element).fontSize);
      if (size < limit) {
        const tag = `${element.tagName.toLowerCase()}.${element.className}`;
        if (!found.some((entry) => entry.tag === tag && entry.size === size)) {
          found.push({ tag, text: text.slice(0, 40), size });
        }
      }
    }
    for (const field of document.querySelectorAll('input, textarea, select')) {
      if (field instanceof HTMLInputElement && ['radio', 'checkbox'].includes(field.type)) {
        continue;
      }
      const size = parseFloat(getComputedStyle(field).fontSize);
      if (!hiddenFromView(field) && size < limit) {
        found.push({ tag: field.tagName.toLowerCase(), text: field.getAttribute('aria-label') ?? '', size });
      }
    }
    return found;
  }, min);
}

for (const screen of SCREENS) {
  for (const size of [WIDE, NARROW]) {
    test(`${screen.name} at ${size.width} px has no visible text under ${MIN_FONT_PX} px`, async ({ page }) => {
      await page.setViewportSize(size);
      await screen.open(page);

      expect(await smallTexts(page, MIN_FONT_PX)).toEqual([]);
    });
  }
}
