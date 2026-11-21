import type { ImageType } from '../body/types';

export interface ImageSize {
  width: number;
  height: number;
}

const PNG_WIDTH_OFFSET = 16;
const PNG_HEIGHT_OFFSET = 20;
const PNG_HEADER_BYTES = 24;
const GIF_WIDTH_OFFSET = 6;
const GIF_HEIGHT_OFFSET = 8;
const GIF_HEADER_BYTES = 10;
const JPEG_START = 2;
const JPEG_MARKER_PREFIX = 0xff;
const JPEG_SOF_HEIGHT_OFFSET = 5;
const JPEG_SOF_WIDTH_OFFSET = 7;
const JPEG_SOF_MIN_BYTES = 9;
const JPEG_STANDALONE_MARKER = 0x01;
const JPEG_RESTART_FIRST = 0xd0;
const JPEG_RESTART_LAST = 0xd7;
const JPEG_START_OF_IMAGE = 0xd8;
const JPEG_END_OF_IMAGE = 0xd9;
const JPEG_START_OF_SCAN = 0xda;
const JPEG_NON_SOF_IN_RANGE = [0xc4, 0xc8, 0xcc];
const JPEG_SOF_FIRST = 0xc0;
const JPEG_SOF_LAST = 0xcf;
const WEBP_CHUNK_OFFSET = 12;
const WEBP_DATA_OFFSET = 20;
const WEBP_LOSSY_START_CODE_OFFSET = 23;
const WEBP_LOSSY_WIDTH_OFFSET = 26;
const WEBP_LOSSY_HEIGHT_OFFSET = 28;
const WEBP_LOSSY_MIN_BYTES = 30;
const WEBP_LOSSLESS_SIGNATURE = 0x2f;
const WEBP_LOSSLESS_BITS_OFFSET = 21;
const WEBP_LOSSLESS_MIN_BYTES = 25;
const WEBP_EXTENDED_WIDTH_OFFSET = 24;
const WEBP_EXTENDED_HEIGHT_OFFSET = 27;
const WEBP_EXTENDED_MIN_BYTES = 30;
const LOSSY_14_BITS = 0x3fff;
const LOSSLESS_HEIGHT_SHIFT = 14;
const LOSSY_START_CODE = [0x9d, 0x01, 0x2a];
const FOURCC_LOSSY = 'VP8 ';
const FOURCC_LOSSLESS = 'VP8L';
const FOURCC_EXTENDED = 'VP8X';

function u16be(bytes: Uint8Array, offset: number): number {
  return ((bytes[offset] ?? 0) << 8) | (bytes[offset + 1] ?? 0);
}

function u16le(bytes: Uint8Array, offset: number): number {
  return (bytes[offset] ?? 0) | ((bytes[offset + 1] ?? 0) << 8);
}

function u24le(bytes: Uint8Array, offset: number): number {
  return u16le(bytes, offset) | ((bytes[offset + 2] ?? 0) << 16);
}

function u32be(bytes: Uint8Array, offset: number): number {
  return u16be(bytes, offset) * 0x10000 + u16be(bytes, offset + 2);
}

function u32le(bytes: Uint8Array, offset: number): number {
  return u16le(bytes, offset) + u16le(bytes, offset + 2) * 0x10000;
}

function fourCc(bytes: Uint8Array, offset: number): string {
  return String.fromCharCode(
    bytes[offset] ?? 0,
    bytes[offset + 1] ?? 0,
    bytes[offset + 2] ?? 0,
    bytes[offset + 3] ?? 0,
  );
}

function pngSize(bytes: Uint8Array): ImageSize | null {
  if (bytes.length < PNG_HEADER_BYTES) {
    return null;
  }
  return { width: u32be(bytes, PNG_WIDTH_OFFSET), height: u32be(bytes, PNG_HEIGHT_OFFSET) };
}

function gifSize(bytes: Uint8Array): ImageSize | null {
  if (bytes.length < GIF_HEADER_BYTES) {
    return null;
  }
  return { width: u16le(bytes, GIF_WIDTH_OFFSET), height: u16le(bytes, GIF_HEIGHT_OFFSET) };
}

function isJpegFrameMarker(marker: number): boolean {
  return marker >= JPEG_SOF_FIRST && marker <= JPEG_SOF_LAST && !JPEG_NON_SOF_IN_RANGE.includes(marker);
}

function isJpegStandalone(marker: number): boolean {
  return (
    marker === JPEG_STANDALONE_MARKER ||
    marker === JPEG_START_OF_IMAGE ||
    (marker >= JPEG_RESTART_FIRST && marker <= JPEG_RESTART_LAST)
  );
}

function jpegSize(bytes: Uint8Array): ImageSize | null {
  let offset = JPEG_START;
  while (offset + 1 < bytes.length) {
    if (bytes[offset] !== JPEG_MARKER_PREFIX) {
      return null;
    }
    const marker = bytes[offset + 1] ?? 0;
    if (marker === JPEG_MARKER_PREFIX) {
      offset += 1;
    } else if (isJpegStandalone(marker)) {
      offset += 2;
    } else if (marker === JPEG_END_OF_IMAGE || marker === JPEG_START_OF_SCAN) {
      return null;
    } else if (isJpegFrameMarker(marker)) {
      if (offset + JPEG_SOF_MIN_BYTES > bytes.length) {
        return null;
      }
      return {
        width: u16be(bytes, offset + JPEG_SOF_WIDTH_OFFSET),
        height: u16be(bytes, offset + JPEG_SOF_HEIGHT_OFFSET),
      };
    } else {
      const length = u16be(bytes, offset + 2);
      if (length < 2) {
        return null;
      }
      offset += 2 + length;
    }
  }
  return null;
}

function webpSize(bytes: Uint8Array): ImageSize | null {
  const chunk = fourCc(bytes, WEBP_CHUNK_OFFSET);
  if (chunk === FOURCC_LOSSY && bytes.length >= WEBP_LOSSY_MIN_BYTES) {
    const startCode = LOSSY_START_CODE.every((value, index) => bytes[WEBP_LOSSY_START_CODE_OFFSET + index] === value);
    return startCode
      ? {
          width: u16le(bytes, WEBP_LOSSY_WIDTH_OFFSET) & LOSSY_14_BITS,
          height: u16le(bytes, WEBP_LOSSY_HEIGHT_OFFSET) & LOSSY_14_BITS,
        }
      : null;
  }
  if (chunk === FOURCC_LOSSLESS && bytes.length >= WEBP_LOSSLESS_MIN_BYTES) {
    if (bytes[WEBP_DATA_OFFSET] !== WEBP_LOSSLESS_SIGNATURE) {
      return null;
    }
    const bits = u32le(bytes, WEBP_LOSSLESS_BITS_OFFSET);
    return {
      width: (bits & LOSSY_14_BITS) + 1,
      height: (Math.floor(bits / 2 ** LOSSLESS_HEIGHT_SHIFT) & LOSSY_14_BITS) + 1,
    };
  }
  if (chunk === FOURCC_EXTENDED && bytes.length >= WEBP_EXTENDED_MIN_BYTES) {
    return {
      width: u24le(bytes, WEBP_EXTENDED_WIDTH_OFFSET) + 1,
      height: u24le(bytes, WEBP_EXTENDED_HEIGHT_OFFSET) + 1,
    };
  }
  return null;
}

export function readImageSize(bytes: Uint8Array, image: ImageType): ImageSize | null {
  switch (image) {
    case 'png':
      return pngSize(bytes);
    case 'gif':
      return gifSize(bytes);
    case 'jpeg':
      return jpegSize(bytes);
    case 'webp':
      return webpSize(bytes);
  }
}

export function pixelCount(size: ImageSize): number {
  return size.width * size.height;
}
