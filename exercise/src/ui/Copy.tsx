import { useState } from 'react';

/** Copy some text, and say so for a moment. */
export function Copy({ text, label = 'copy' }: { readonly text: string; readonly label?: string }) {
  const [done, setDone] = useState(false);
  return (
    <button
      type="button"
      className="ex-action"
      data-done={done ? '' : undefined}
      onClick={() => {
        void navigator.clipboard?.writeText(text).then(() => {
          setDone(true);
          setTimeout(() => setDone(false), 1400);
        });
      }}
    >
      {done ? 'copied' : label}
    </button>
  );
}
