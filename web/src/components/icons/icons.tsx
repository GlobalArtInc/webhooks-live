import type { ReactNode } from 'react';

export interface IconProps {
  size?: number;
}

interface GlyphProps extends IconProps {
  children: ReactNode;
}

const DEFAULT_SIZE = 24;
const STROKE_WIDTH = 1.75;

function Glyph({ size = DEFAULT_SIZE, children }: GlyphProps) {
  return (
    <svg
      aria-hidden="true"
      focusable="false"
      width={size}
      height={size}
      viewBox="0 0 24 24"
      fill="none"
      stroke="currentColor"
      strokeWidth={STROKE_WIDTH}
      strokeLinecap="round"
      strokeLinejoin="round"
    >
      {children}
    </svg>
  );
}

export function Plus(props: IconProps) {
  return (
    <Glyph {...props}>
      <path d="M12 5v14M5 12h14" />
    </Glyph>
  );
}

export function ChevronsRight(props: IconProps) {
  return (
    <Glyph {...props}>
      <path d="m6 17 5-5-5-5M13 17l5-5-5-5" />
    </Glyph>
  );
}

export function ChevronsLeft(props: IconProps) {
  return (
    <Glyph {...props}>
      <path d="m18 17-5-5 5-5M11 17l-5-5 5-5" />
    </Glyph>
  );
}

export function List(props: IconProps) {
  return (
    <Glyph {...props}>
      <path d="M9 6h11M9 12h11M9 18h11M4.5 6h.01M4.5 12h.01M4.5 18h.01" />
    </Glyph>
  );
}

export function Send(props: IconProps) {
  return (
    <Glyph {...props}>
      <path d="M21 3 10 14M21 3l-7 18-4-7-7-4 18-7Z" />
    </Glyph>
  );
}

export function Folder(props: IconProps) {
  return (
    <Glyph {...props}>
      <path d="M3 7a2 2 0 0 1 2-2h4l2 2h8a2 2 0 0 1 2 2v8a2 2 0 0 1-2 2H5a2 2 0 0 1-2-2Z" />
    </Glyph>
  );
}

export function Tag(props: IconProps) {
  return (
    <Glyph {...props}>
      <path d="M20.6 13.4 13.4 20.6a2 2 0 0 1-2.8 0L3 13V3h10l7.6 7.6a2 2 0 0 1 0 2.8ZM7.5 7.5h.01" />
    </Glyph>
  );
}

export function Copy(props: IconProps) {
  return (
    <Glyph {...props}>
      <path d="M9 9h10v10H9zM5 15V5h10" />
    </Glyph>
  );
}
