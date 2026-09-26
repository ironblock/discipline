import { Lexer } from 'marked';
import type { MarkedToken, Token } from 'marked';
import { Fragment, Profiler, createContext, memo, useContext, useMemo } from 'react';
import type { ReactNode } from 'react';

import { Copy } from './Copy.tsx';
import './prose.css';

/**
 * Model or person prose, as the Markdown chat answers use: CommonMark with
 * GitHub's tables, task lists, strikethrough and bare links, lexed by
 * `marked` into tokens and built here as React elements -- never as HTML, so
 * nothing a model says becomes markup. What it refuses: raw HTML (shown as
 * the text it is), links that are not http, https or mailto (their text,
 * unlinked), and images (a link to the image; nothing is fetched).
 *
 * While streaming, each top-level block keeps its identity by its source:
 * a finished block is never rendered again, so an update costs the growing
 * block, not the answer.
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
  const blocks = useMemo(() => Lexer.lex(text, { gfm: true }).filter((t) => t.type !== 'space' && t.type !== 'def'), [text]);
  return (
    <div className={`ex-prose ex-prose--${kind}`}>
      {blocks.map((token, i) => (
        <BlockOf key={i} token={token} raw={token.raw} caret={caret && i === blocks.length - 1} />
      ))}
      {caret && blocks.length === 0 ? <Caret /> : null}
    </div>
  );
}

/** Test seam: called each time a top-level block renders. Stories count them; nothing else provides it. */
export const ProseProbe = createContext<(() => void) | undefined>(undefined);

const BlockOf = memo(
  function BlockOf({ token, caret }: { readonly token: Token; readonly raw: string; readonly caret: boolean }) {
    const probe = useContext(ProseProbe);
    const element = block(token as MarkedToken, caret);
    return probe ? (
      <Profiler id="prose-block" onRender={probe}>
        {element}
      </Profiler>
    ) : (
      element
    );
  },
  (before, after) => before.raw === after.raw && before.caret === after.caret,
);

function Caret() {
  return <span className="ex-caret" aria-hidden="true" />;
}

/** A block, top-level or nested. `caret` lands inside it, at the end of its last line. */
function block(token: MarkedToken, caret: boolean): ReactNode {
  const end = caret ? <Caret /> : null;
  switch (token.type) {
    case 'paragraph':
      return (
        <p>
          {inline(token.tokens)}
          {end}
        </p>
      );
    case 'text':
      // A tight list item's text: inline content, no paragraph.
      return (
        <>
          {token.tokens ? inline(token.tokens) : decode(token.text)}
          {end}
        </>
      );
    case 'heading':
      // A heading in an answer is a section label, not a page title.
      return (
        <p className="ex-prose__heading" role="heading" aria-level={Math.min(6, token.depth + 2)} data-depth={token.depth}>
          {inline(token.tokens)}
          {end}
        </p>
      );
    case 'code':
      return (
        <div className="ex-prose__codebox">
          <pre className="ex-prose__code" data-lang={token.lang || undefined}>
            {token.text}
            {end}
          </pre>
          <span className="ex-prose__copy">
            <Copy text={token.text} />
          </span>
        </div>
      );
    case 'list': {
      const List = token.ordered ? 'ol' : 'ul';
      const start = token.ordered && typeof token.start === 'number' && token.start !== 1 ? token.start : undefined;
      const last = token.items.length - 1;
      return (
        <List start={start} data-loose={token.loose ? '' : undefined}>
          {token.items.map((item, i) => (
            <li key={i} data-task={item.task ? '' : undefined}>
              {item.tokens.map((child, j) => (
                <Fragment key={j}>{block(child as MarkedToken, caret && i === last && j === item.tokens.length - 1)}</Fragment>
              ))}
              {caret && i === last && item.tokens.length === 0 ? <Caret /> : null}
            </li>
          ))}
        </List>
      );
    }
    case 'checkbox':
      return <input className="ex-prose__task" type="checkbox" checked={token.checked} disabled readOnly />;
    case 'table':
      return (
        <div className="ex-prose__table">
          <table>
            <thead>
              <tr>
                {token.header.map((cell, i) => (
                  <th key={i} data-align={cell.align ?? undefined}>
                    {inline(cell.tokens)}
                  </th>
                ))}
              </tr>
            </thead>
            <tbody>
              {token.rows.map((row, r) => (
                <tr key={r}>
                  {row.map((cell, i) => (
                    <td key={i} data-align={cell.align ?? undefined}>
                      {inline(cell.tokens)}
                    </td>
                  ))}
                </tr>
              ))}
            </tbody>
          </table>
          {end}
        </div>
      );
    case 'blockquote':
      return (
        <blockquote>
          {token.tokens.map((child, i) => (
            <Fragment key={i}>{block(child as MarkedToken, caret && i === token.tokens.length - 1)}</Fragment>
          ))}
        </blockquote>
      );
    case 'hr':
      return <hr />;
    case 'html':
      // Raw HTML is shown as the text it is.
      return (
        <p>
          {token.text}
          {end}
        </p>
      );
    case 'space':
    case 'def':
      return end;
    default:
      // Anything else, whatever a later lexer adds, is shown as its source.
      return (
        <p>
          {token.raw}
          {end}
        </p>
      );
  }
}

function inline(tokens: readonly Token[]): ReactNode[] {
  return tokens.map((token, i) => <Fragment key={i}>{span(token as MarkedToken)}</Fragment>);
}

function span(token: MarkedToken): ReactNode {
  switch (token.type) {
    case 'text':
      return token.tokens ? inline(token.tokens) : decode(token.text);
    case 'escape':
      return token.text;
    case 'strong':
      return <strong>{inline(token.tokens)}</strong>;
    case 'em':
      return <em>{inline(token.tokens)}</em>;
    case 'del':
      return <del>{inline(token.tokens)}</del>;
    case 'codespan':
      return <code>{token.text}</code>;
    case 'br':
      return <br />;
    case 'link':
      return linked(token.href, token.title, inline(token.tokens));
    case 'image':
      // Never fetched: a link to the image, named by its description.
      return linked(token.href, token.title, <span className="ex-prose__image">image: {decode(token.text) || token.href}</span>);
    case 'html':
      // Raw HTML is shown as the text it is.
      return token.text;
    default:
      return token.raw;
  }
}

const SAFE_LINK = /^(https?:|mailto:)/i;

function linked(href: string, title: string | null | undefined, children: ReactNode): ReactNode {
  if (!SAFE_LINK.test(href.trim())) return children;
  return (
    <a href={href} title={title ? decode(title) : undefined} target="_blank" rel="noopener noreferrer">
      {children}
    </a>
  );
}

/** Character references in text (`&amp;`, `&copy;`, `&#233;`) as the characters they name; anything else as written. */
const REFERENCE = /&(#[0-9]{1,7}|#[xX][0-9a-fA-F]{1,6}|[A-Za-z][A-Za-z0-9]{1,31});/g;
const decoded = new Map<string, string>();

function decode(text: string): string {
  if (!text.includes('&')) return text;
  return text.replace(REFERENCE, (reference) => {
    let character = decoded.get(reference);
    if (character === undefined) {
      // The reference alone -- the pattern admits no `<`, so no markup reaches
      // here -- read by the browser, which knows every name HTML does.
      const scratch = document.createElement('textarea');
      scratch.innerHTML = reference;
      character = scratch.value;
      decoded.set(reference, character);
    }
    return character;
  });
}
