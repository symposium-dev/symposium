import { SyntaxHighlighter } from '../syntax-highlighter';

describe('syntax-highlighter', () => {
  it('render', () => {
    const testSyntaxHighlighter = new SyntaxHighlighter({
      codeStringWithMarkup: 'alert("hello");\n',
      language: 'js',
      block: true,
    });

    expect(testSyntaxHighlighter.render.outerHTML.replace('\n', '')).toBe(
      '<div class="mynah-syntax-highlighter"><pre><code class="language-js hljs language-javascript" data-highlighted="yes"><span class="hljs-title function_">alert</span>(<span class="hljs-string">"hello"</span>);</code></pre><div class="more-content-indicator"><button class="mynah-button mynah-button-secondary fill-state-hover mynah-ui-clickable-item" tabindex="0"><i class="mynah-ui-icon mynah-ui-icon-down-open"></i></button></div><div class="mynah-syntax-highlighter-copy-buttons"><span class="mynah-syntax-highlighter-language">js</span></div></div>'
    );
  });

  it('should show buttons if showCopyButtons true and related events are connected', () => {
    const testSyntaxHighlighter = new SyntaxHighlighter({
      codeStringWithMarkup: 'alert("hello");\n',
      language: 'typescript',
      codeBlockActions: {
        copy: {
          id: 'copy',
          label: 'Copy'
        },
        'insert-at-cursor': {
          id: 'insert-at-cursor',
          label: 'Insert at cursor'
        },
      },
      onCopiedToClipboard: () => {},
      onCodeBlockAction: () => {},
      block: true,
    });
    expect(testSyntaxHighlighter.render.querySelectorAll('button')?.length).toBe(3);
    expect(testSyntaxHighlighter.render.querySelectorAll('button')?.[1]?.textContent).toBe('Copy');
    expect(testSyntaxHighlighter.render.querySelectorAll('button')?.[2]?.textContent).toBe('Insert at cursor');
  });
});
