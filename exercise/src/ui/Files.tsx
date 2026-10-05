import { createContext, useContext, useEffect, useState } from 'react';

import { read } from '../drive/files.ts';
import type { Checked, FileSource } from '../drive/files.ts';
import type { FileRef } from '../drive/log.ts';
import { size } from './format.ts';
import './files.css';

/** Where this session's files are read from, by digest (#372). Absent: none can be. */
export const FilesContext = createContext<FileSource | undefined>(undefined);

/**
 * The files a tool call's result is (#372): each by its reference -- path,
 * media type, size, digest -- and an image drawn from its bytes once they hash
 * to its digest (`files.ts`), never from its path. What cannot be drawn says
 * why, under the reference: withheld from the publication, not found, bytes
 * that are not the digest, or a source that cannot be asked.
 */
export function FileResults({ files }: { readonly files: readonly FileRef[] }) {
  return (
    <ul className="ex-files" aria-label="the files it wrote">
      {files.map((file) => (
        <li key={`${file.sha256}/${file.path}`} className="ex-file">
          {file.media_type.startsWith('image/') ? <CheckedImage file={file} /> : null}
          <span className="ex-file__ref" title={`sha256 ${file.sha256}`}>
            {file.path} · {file.media_type} · {size(file.bytes)} · {file.sha256.slice(0, 12)}
          </span>
        </li>
      ))}
    </ul>
  );
}

type Reading = { readonly kind: 'reading' } | { readonly kind: 'unreadable' } | Checked;

/** What the surface says of a file it does not draw: the word, and the digest where the digest is the point. */
function said(reading: Exclude<Reading, { readonly kind: 'shown' }>, file: FileRef): string {
  switch (reading.kind) {
    case 'reading':
      return 'reading…';
    case 'unreadable':
      return 'not readable here: this session has no files';
    case 'withheld':
      return 'withheld: its recording does not declare it clean, so it is not published';
    case 'not-found':
      return `refused: no file ${file.sha256}`;
    case 'mismatch':
      return `refused: the bytes are not ${file.sha256} (they are ${reading.got})`;
    case 'unreachable':
      return `not read: ${reading.why}`;
  }
}

function CheckedImage({ file }: { readonly file: FileRef }) {
  const source = useContext(FilesContext);
  const [reading, setReading] = useState<Reading>(source ? { kind: 'reading' } : { kind: 'unreadable' });
  const [url, setUrl] = useState<string | undefined>(undefined);
  useEffect(() => {
    if (!source) return setReading({ kind: 'unreadable' });
    let live = true;
    let made: string | undefined;
    setReading({ kind: 'reading' });
    void read(file, source).then((checked) => {
      if (!live) return;
      if (checked.kind === 'shown') {
        made = URL.createObjectURL(new Blob([checked.bytes as Uint8Array<ArrayBuffer>], { type: file.media_type }));
        setUrl(made);
      }
      setReading(checked);
    });
    return () => {
      live = false;
      if (made) URL.revokeObjectURL(made);
      setUrl(undefined);
    };
  }, [file, source]);
  if (reading.kind === 'shown' && url) return <img className="ex-file__image" src={url} alt={file.path} data-sha256={file.sha256} />;
  return (
    <span className="ex-file__unshown" data-reading={reading.kind} role={reading.kind === 'not-found' || reading.kind === 'mismatch' ? 'alert' : undefined}>
      {reading.kind === 'shown' ? 'reading…' : said(reading, file)}
    </span>
  );
}
