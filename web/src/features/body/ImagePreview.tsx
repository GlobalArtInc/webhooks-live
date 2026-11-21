import { useEffect, useRef, useState } from 'react';
import { formatSize } from './bytes';
import { imageMimeType } from './classify';
import styles from './ImagePreview.module.css';
import type { Bytes, ImageType } from './types';

interface ImagePreviewProps {
  bytes: Bytes;
  image: ImageType;
}

export function ImagePreview({ bytes, image }: ImagePreviewProps) {
  const imageRef = useRef<HTMLImageElement>(null);
  const [failed, setFailed] = useState(false);

  useEffect(() => {
    const element = imageRef.current;
    if (element === null) {
      return undefined;
    }
    const url = URL.createObjectURL(new Blob([bytes], { type: imageMimeType(image) }));
    element.src = url;
    return () => {
      element.removeAttribute('src');
      URL.revokeObjectURL(url);
    };
  }, [bytes, image]);

  return (
    <figure className={styles.figure}>
      {failed ? (
        <p className={styles.failure} role="status">
          The {image.toUpperCase()} data could not be decoded as an image. Use Download raw to inspect it.
        </p>
      ) : (
        <img
          ref={imageRef}
          className={styles.image}
          alt={`Preview of the captured ${image.toUpperCase()} image`}
          decoding="async"
          referrerPolicy="no-referrer"
          onError={() => {
            setFailed(true);
          }}
        />
      )}
      <figcaption className={styles.caption}>
        {image.toUpperCase()} image, {formatSize(bytes.length)}
      </figcaption>
    </figure>
  );
}
