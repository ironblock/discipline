import { useEffect, useState } from 'react';

/** Copy some text, and say so for a moment -- or that it could not. */
export function Copy({ text, label = 'copy' }: { readonly text: string; readonly label?: string }) {
  const [said, setSaid] = useState<'copied' | 'not copied' | undefined>(undefined);
  useEffect(() => {
    if (!said) return;
    const id = setTimeout(() => setSaid(undefined), 1400);
    return () => clearTimeout(id);
  }, [said]);
  return (
    <button
      type="button"
      className="ex-action"
      data-done={said === 'copied' ? '' : undefined}
      onClick={() => {
        const write = navigator.clipboard?.writeText(text);
        if (!write) return setSaid('not copied');
        write.then(
          () => setSaid('copied'),
          () => setSaid('not copied'),
        );
      }}
    >
      {said ?? label}
    </button>
  );
}
