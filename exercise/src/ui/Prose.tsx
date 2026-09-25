import type { ReactNode } from 'react';

import './prose.css';

/**
 * Model or person prose, with the little Markdown chat answers use:
 * paragraphs, `-` and `1.` lists, inline code, fenced code. Built as elements,
 * never as HTML, so nothing a model says can become markup.
 */
export function Prose({
  text,
  kind,
  caret = false,
}: {
  readonly text: string;
  readonly kind: 'ask' | 'answer' | 'reasoning' | 'system';
  /** Streaming: a caret at the point the text is growing from. */
  readonly caret?: boolean;
}) {
  return (
    <div className={`ex-prose ex-prose--${kind}`}>
      {blocks(text)}
      {caret ? <span className="ex-caret" aria-hidden="true" /> : null}
    </div>
  );
}

function blocks(text: string): ReactNode[] {
  const out: ReactNode[] = [];
  const lines = text.split('\n');
  let i = 0;
  let key = 0;
  while (i < lines.length) {
    const line = lines[i] ?? '';
    if (line.startsWith('```')) {
      const body: string[] = [];
      i += 1;
      while (i < lines.length && !(lines[i] ?? '').startsWith('```')) body.push(lines[i++] ?? '');
      i += 1;
      out.push(
        <pre className="ex-prose__code" key={key++}>
          {body.join('\n')}
        </pre>,
      );
      continue;
    }
    const bullet = /^\s*[-*] (.*)$/;
    const numbered = /^\s*\d+\. (.*)$/;
    if (bullet.test(line) || numbered.test(line)) {
      const pattern = bullet.test(line) ? bullet : numbered;
      const items: string[] = [];
      while (i < lines.length && pattern.test(lines[i] ?? '')) items.push((lines[i++] ?? '').replace(pattern, '$1'));
      const List = pattern === bullet ? 'ul' : 'ol';
      out.push(
        <List key={key++}>
          {items.map((item, n) => (
            <li key={n}>{inline(item)}</li>
          ))}
        </List>,
      );
      continue;
    }
    if (line.trim() === '') {
      i += 1;
      continue;
    }
    const para: string[] = [];
    while (i < lines.length && (lines[i] ?? '').trim() !== '' && !/^(```|\s*[-*] |\s*\d+\. )/.test(lines[i] ?? '')) para.push(lines[i++] ?? '');
    out.push(<p key={key++}>{inline(para.join('\n'))}</p>);
  }
  return out;
}

function inline(text: string): ReactNode[] {
  return text.split(/(`[^`]+`)/g).map((part, i) =>
    part.startsWith('`') && part.endsWith('`') && part.length > 1 ? <code key={i}>{part.slice(1, -1)}</code> : part,
  );
}
