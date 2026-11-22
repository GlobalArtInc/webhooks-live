export interface TextSegment {
  text: string;
  code: boolean;
}

export type TextBlock = TextSegment[];

const PARAGRAPH_BREAK = /\n\s*\n/;

function segmentsOf(paragraph: string): TextSegment[] {
  const parts = paragraph.split('`');
  const closed = parts.length % 2 === 1;
  const segments: TextSegment[] = [];
  parts.forEach((part, index) => {
    if (part === '') {
      return;
    }
    const code = index % 2 === 1 && (closed || index < parts.length - 1);
    segments.push({ text: code ? part : index % 2 === 1 ? `\`${part}` : part, code });
  });
  return segments;
}

export function textBlocks(text: string | null): TextBlock[] {
  if (text === null) {
    return [];
  }
  return text
    .split(PARAGRAPH_BREAK)
    .map((paragraph) => paragraph.replace(/\s*\n\s*/g, ' ').trim())
    .filter((paragraph) => paragraph !== '')
    .map(segmentsOf);
}

export function plainText(text: string | null): string {
  return textBlocks(text)
    .map((block) => block.map((segment) => segment.text).join(''))
    .join(' ');
}
