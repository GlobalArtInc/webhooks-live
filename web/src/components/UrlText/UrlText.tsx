import { Fragment } from 'react';

const AFTER_SEPARATOR = /(?<=[/?&=])/;

export interface UrlTextProps {
  url: string;
}

export function breakUrl(url: string): string[] {
  return url.split(AFTER_SEPARATOR);
}

export function UrlText({ url }: UrlTextProps) {
  return (
    <>
      {breakUrl(url).map((part, index) => (
        <Fragment key={index}>
          {index === 0 ? null : <wbr />}
          {part}
        </Fragment>
      ))}
    </>
  );
}
