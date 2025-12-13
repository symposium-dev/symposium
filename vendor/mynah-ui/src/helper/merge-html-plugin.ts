/*
  Highlight.js does not support unescaped HTML by default to prevent XSS.
  This plugin allows this, so that we can implement highlights with tooltips.

  Taken from: https://github.com/highlightjs/highlight.js/issues/2889
*/

import { HighlightResult, HLJSPlugin } from 'highlight.js';

export const mergeHTMLPlugin = (function () {
  let originalStream: Event[];

  function escapeHTML (value: string): string {
    return value
      .replace(/&/g, '&amp;')
      .replace(/</g, '&lt;')
      .replace(/>/g, '&gt;')
      .replace(/"/g, '&quot;')
      .replace(/'/g, '&#x27;');
  }

  const mergeHTMLPlugin: HLJSPlugin = {
    'before:highlightElement': ({ el }: {el: Node}) => {
      originalStream = nodeStream(el);
    },
    'after:highlightElement': ({ el, result, text }: {el: Element; result: HighlightResult; text: string}) => {
      if (originalStream.length === 0) return;

      const resultNode = document.createElement('div');
      resultNode.innerHTML = result.value;
      result.value = mergeStreams(originalStream, nodeStream(resultNode), text);
      el.innerHTML = result.value;
    }
  };

  interface Event {
    event: 'start' | 'stop';
    offset: number;
    node: Node;
  }

  function tag (node: Node): string {
    return node.nodeName.toLowerCase();
  }

  function nodeStream (node: Node): Event[] {
    const result: Event[] = [];
    (function _nodeStream (node, offset) {
      for (let child = node.firstChild; child != null; child = child.nextSibling) {
        if (child.nodeType === 3) {
          offset += child.nodeValue?.length ?? 0;
        } else if (child.nodeType === 1) {
          result.push({
            event: 'start',
            offset,
            node: child
          });
          offset = _nodeStream(child, offset);
          if (tag(child).match(/br|hr|img|input/) == null) {
            result.push({
              event: 'stop',
              offset,
              node: child
            });
          }
        }
      }
      return offset;
    })(node, 0);
    return result;
  }

  function mergeStreams (original: Event[], highlighted: Event[], value: string): string {
    let processed = 0;
    let result = '';
    const nodeStack = [];

    function selectStream (): Event[] {
      if ((original.length === 0) || (highlighted.length === 0)) {
        return (original.length > 0) ? original : highlighted;
      }
      if (original[0].offset !== highlighted[0].offset) {
        return (original[0].offset < highlighted[0].offset) ? original : highlighted;
      }

      return highlighted[0].event === 'start' ? original : highlighted;
    }

    function open (node: Node): void {
      function attributeString (attr: Attr): string {
        return ' ' + attr.nodeName + '="' + escapeHTML(attr.value) + '"';
      }
      // @ts-expect-error
      result += '<' + tag(node) + [].map.call(node.attributes, attributeString).join('') + '>';
    }

    function close (node: Node): void {
      result += '</' + tag(node) + '>';
    }

    function render (event: Event): void {
      (event.event === 'start' ? open : close)(event.node);
    }

    while ((original.length > 0) || (highlighted.length > 0)) {
      let stream = selectStream();
      result += escapeHTML(value.substring(processed, stream[0].offset));
      processed = stream[0].offset;
      if (stream === original) {
        nodeStack.reverse().forEach(close);
        do {
          render(stream.splice(0, 1)[0]);
          stream = selectStream();
        } while (stream === original && (stream.length > 0) && stream[0].offset === processed);
        nodeStack.reverse().forEach(open);
      } else {
        if (stream[0].event === 'start') {
          nodeStack.push(stream[0].node);
        } else {
          nodeStack.pop();
        }
        render(stream.splice(0, 1)[0]);
      }
    }
    return result + escapeHTML(value.substr(processed));
  }

  return mergeHTMLPlugin;
}());
