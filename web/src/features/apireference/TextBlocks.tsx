import { textBlocks } from './text';
import styles from './TextBlocks.module.css';

export interface TextBlocksProps {
  text: string | null;
}

export function TextBlocks({ text }: TextBlocksProps) {
  return (
    <>
      {textBlocks(text).map((block, blockIndex) => (
        <p key={blockIndex} className={styles.paragraph}>
          {block.map((segment, segmentIndex) =>
            segment.code ? (
              <code key={segmentIndex} className={styles.code}>
                {segment.text}
              </code>
            ) : (
              segment.text
            ),
          )}
        </p>
      ))}
    </>
  );
}
