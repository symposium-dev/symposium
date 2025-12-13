/*!
 * Copyright 2022 Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

import sanitizeHtml from 'sanitize-html';

export const AllowedTags = [
  'a',
  'audio',
  'b',
  'blockquote',
  'br',
  'hr',
  'canvas',
  'code',
  'col',
  'colgroup',
  'data',
  'div',
  'em',
  'embed',
  'figcaption',
  'figure',
  'h1',
  'h2',
  'h3',
  'h4',
  'h5',
  'h6',
  'i',
  'iframe',
  'img',
  'input',
  'li',
  'map',
  'mark',
  'object',
  'ol',
  'p',
  'pre',
  'q',
  's',
  'small',
  'source',
  'span',
  'strong',
  'sub',
  'sup',
  'table',
  'tbody',
  'td',
  'tfoot',
  'th',
  'thead',
  'tr',
  'track',
  'u',
  'ul',
  'video',
] as const;

export const AllowedAttributes = [
  'accept',
  'accept-charset',
  'accesskey',
  'align',
  'allow',
  'allowfullscreen',
  'alt',
  'as',
  'async',
  'autocapitalize',
  'autoplay',
  'charset',
  'class',
  'cols',
  'colspan',
  'controls',
  'crossorigin',
  'data',
  'data-*',
  'datetime',
  'decoding',
  'default',
  'dir',
  'download',
  'headers',
  'hidden',
  'high',
  'href',
  'hreflang',
  'id',
  'ismap',
  'itemprop',
  'kind',
  'lang',
  'language',
  'loop',
  'low',
  'media',
  'muted',
  'optimum',
  'ping',
  'playsinline',
  'poster',
  'preload',
  'referrerpolicy',
  'rel',
  'reversed',
  'role',
  'rowspan',
  'sandbox',
  'scope',
  'shape',
  'size',
  'sizes',
  'slot',
  'span',
  'spellcheck',
  'src',
  'srcdoc',
  'srclang',
  'srcset',
  'start',
  'style',
  'target',
  'title',
  'translate',
  'usemap',
  'wrap',
  'aspect-ratio',
] as const;

export type AllowedTagsInCustomRenderer = (typeof AllowedTags)[number];
export type AllowedAttributesInCustomRenderer = (typeof AllowedAttributes)[number];

export const cleanHtml = (dirty: string): string => {
  return sanitizeHtml(dirty, {
    allowedTags: [ ...AllowedTags ],
    allowedAttributes: {
      '*': [ ...AllowedAttributes ],
      input: [ 'type', 'disabled', 'checked' ]
    },
    transformTags: {
      input: ((tagName, attribs) => {
        // Only allow input if it's a checkbox and disabled
        if (attribs.type === 'checkbox' && attribs.disabled != null) {
          return {
            tagName,
            attribs: {
              type: 'checkbox',
              disabled: 'disabled',
              checked: attribs.checked
            }
          };
        }
        // For all other inputs, remove the tag
        return {
          tagName: '',
          attribs: {}
        };
      }) as sanitizeHtml.Transformer
    }
  });
};
