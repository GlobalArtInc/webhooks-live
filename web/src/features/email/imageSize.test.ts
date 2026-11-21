import { describe, expect, it } from 'vitest';
import { pixelCount, readImageSize } from './imageSize';
import { craftGif, craftJpeg, craftPng, craftWebp } from './testFixtures';

describe('readImageSize', () => {
  it('reads the size declared in a png, gif and jpeg header', () => {
    expect(readImageSize(craftPng(4000, 3000), 'png')).toEqual({ width: 4000, height: 3000 });
    expect(readImageSize(craftGif(640, 480), 'gif')).toEqual({ width: 640, height: 480 });
    expect(readImageSize(craftJpeg(8000, 6000), 'jpeg')).toEqual({ width: 8000, height: 6000 });
  });

  it('reads the size of lossy, lossless and extended webp', () => {
    expect(readImageSize(craftWebp('lossy', 1200, 800), 'webp')).toEqual({ width: 1200, height: 800 });
    expect(readImageSize(craftWebp('lossless', 16384, 2), 'webp')).toEqual({ width: 16384, height: 2 });
    expect(readImageSize(craftWebp('extended', 20000, 30000), 'webp')).toEqual({ width: 20000, height: 30000 });
  });

  it('gives up on truncated or malformed headers instead of guessing', () => {
    expect(readImageSize(craftPng(1, 1).subarray(0, 20), 'png')).toBeNull();
    expect(readImageSize(craftGif(1, 1).subarray(0, 8), 'gif')).toBeNull();
    expect(readImageSize(Uint8Array.from([0xff, 0xd8, 0xff, 0xda, 0, 2]), 'jpeg')).toBeNull();
    expect(readImageSize(Uint8Array.from([0xff, 0xd8, 0x00, 0x01]), 'jpeg')).toBeNull();
    expect(readImageSize(craftWebp('lossy', 1, 1).subarray(0, 20), 'webp')).toBeNull();
  });

  it('counts pixels', () => {
    expect(pixelCount({ width: 4000, height: 4000 })).toBe(16_000_000);
  });
});
