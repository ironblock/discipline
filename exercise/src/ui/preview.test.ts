import { describe, expect, it } from 'vitest';

import { preview } from './Message.tsx';

describe('a system prompt preview', () => {
  it('keeps the first lines when nothing is cut mid-block', () => {
    expect(preview('a\nb\nc\nd', 3)).toEqual({ shown: 'a\nb\nc', hidden: 1 });
  });

  it('never ends inside a code block it opened', () => {
    const text = 'You have one tool: bash.\nReply like this:\n```bash\n<one command>\n```\nThen stop.';
    expect(preview(text, 3)).toEqual({ shown: 'You have one tool: bash.\nReply like this:', hidden: 4 });
  });

  it('shows a code block the text opens with whole, not cut inside it', () => {
    expect(preview('```\nfoo\nbar\nbaz\n```\nThen stop.', 3)).toEqual({ shown: '```\nfoo\nbar\nbaz\n```', hidden: 1 });
    expect(preview('```\nfoo\nbar\nbaz', 3)).toEqual({ shown: '```\nfoo\nbar\nbaz', hidden: 0 });
  });

  it('keeps a code block it closed', () => {
    expect(preview('```\nx\n```\ny', 3)).toEqual({ shown: '```\nx\n```', hidden: 1 });
  });
});
