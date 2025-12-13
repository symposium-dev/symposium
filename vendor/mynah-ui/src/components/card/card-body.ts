/*!
 * Copyright 2022 Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

import { cleanupElement, DomBuilder, DomBuilderObject, ExtendedHTMLElement } from '../../helper/dom';
import {
  CodeBlockActions,
  OnCodeBlockActionFunction,
  OnCopiedToClipboardFunction,
  ReferenceTrackerInformation,
} from '../../static';
import unescapeHTML from 'unescape-html';
import { Overlay, OverlayHorizontalDirection, OverlayVerticalDirection } from '../overlay';
import { SyntaxHighlighter } from '../syntax-highlighter';
import { generateUID } from '../../helper/guid';
import { Config } from '../../helper/config';
import { parseMarkdown } from '../../helper/marked';
import { StyleLoader } from '../../helper/style-loader';

const PREVIEW_DELAY = 500;

export const highlightersWithTooltip = {
  start: {
    markupStart: '<mark ',
    markupAttributes: (markerIndex: string) => `marker-index=${markerIndex}`,
    markupEnd: ' reference-tracker>'
  },
  end: {
    markup: '</mark>',
  },
};

export const PARTS_CLASS_NAME = 'typewriter-part';
export const PARTS_CLASS_NAME_VISIBLE = 'typewriter';

export interface CardBodyProps {
  body?: string;
  testId?: string;
  children?: Array<ExtendedHTMLElement | HTMLElement | string | DomBuilderObject>;
  childLocation?: 'above-body' | 'below-body';
  highlightRangeWithTooltip?: ReferenceTrackerInformation[] | null;
  hideCodeBlockLanguage?: boolean;
  wrapCode?: boolean;
  unlimitedCodeBlockHeight?: boolean;
  codeBlockActions?: CodeBlockActions;
  useParts?: boolean;
  codeBlockStartIndex?: number;
  processChildren?: boolean;
  classNames?: string[];
  onLinkClick?: (url: string, e: MouseEvent) => void;
  onCopiedToClipboard?: OnCopiedToClipboardFunction;
  onCodeBlockAction?: OnCodeBlockActionFunction;
}
export class CardBody {
  render: ExtendedHTMLElement;
  props: CardBodyProps;
  nextCodeBlockIndex: number = 0;
  codeBlockStartIndex: number = 0;
  private highlightRangeTooltip: Overlay | null;
  private highlightRangeTooltipTimeout: ReturnType<typeof setTimeout>;
  constructor (props: CardBodyProps) {
    StyleLoader.getInstance().load('components/card/_card.scss');
    this.codeBlockStartIndex = props.codeBlockStartIndex ?? 0;
    this.props = props;
    const bodyChildren = this.getContentBodyChildren(this.props);
    const childList = [
      ...bodyChildren,
      ...(this.props.children != null
        ? this.props.processChildren === true
          ? this.props.children.map((node, index) => {
            const processedNode = this.processNode(node as HTMLElement);
            processedNode.setAttribute?.('render-index', (bodyChildren.length + index).toString());
            cleanupElement(processedNode);
            return processedNode;
          })
          : this.props.children.map((node, index): HTMLElement => {
            (node as HTMLElement)?.setAttribute?.('render-index', (bodyChildren.length + index).toString());
            return node as HTMLElement;
          })
        : [])
    ];
    this.render = DomBuilder.getInstance().build({
      type: 'div',
      testId: this.props.testId,
      classNames: [ 'mynah-card-body', ...(this.props.classNames ?? []) ],
      children: this.props.childLocation === 'above-body' ? childList.reverse() : childList,
    });
    cleanupElement(this.render);

    Array.from(this.render.querySelectorAll('mark[reference-tracker]')).forEach((highlightRangeElement) => {
      highlightRangeElement.addEventListener('mouseenter', (e) => {
        const index = parseInt((e.target as HTMLElement).getAttribute('marker-index') ?? '0');
        if (props.highlightRangeWithTooltip?.[index] !== undefined) {
          this.showHighlightRangeTooltip(e as MouseEvent, props.highlightRangeWithTooltip[index].information);
        }
      });
      highlightRangeElement.addEventListener('mouseleave', this.hideHighlightRangeTooltip);
    });
  }

  private readonly processNode = (node: HTMLElement): HTMLElement => {
    let elementFromNode: HTMLElement = node;
    if (this.props.useParts === true && elementFromNode.nodeType === Node.TEXT_NODE && elementFromNode.textContent?.trim() !== '') {
      elementFromNode = DomBuilder.getInstance().build({
        type: 'span',
        classNames: [ 'mynah-ui-animation-text-content' ],
        children: elementFromNode.textContent?.split(' ').map(textPart => DomBuilder.getInstance().build({
          type: 'span',
          classNames: [ PARTS_CLASS_NAME ],
          children: [ textPart, ' ' ]
        }))
      });
    } else {
      if (elementFromNode.tagName?.toLowerCase() === 'a') {
        const url = elementFromNode.getAttribute('href') ?? '';
        return DomBuilder.getInstance().build(
          {
            type: 'a',
            classNames: this.props.useParts === true ? [ PARTS_CLASS_NAME ] : [],
            events: {
              click: (e: MouseEvent) => {
                if (this.props.onLinkClick !== undefined) {
                  this.props.onLinkClick(url, e);
                }
              },
              auxclick: (e: MouseEvent) => {
                if (this.props.onLinkClick !== undefined) {
                  this.props.onLinkClick(url, e);
                }
              },
            },
            attributes: { href: elementFromNode.getAttribute('href') ?? '', target: '_blank' },
            innerHTML: elementFromNode.innerHTML,
          });
      }
      if ((elementFromNode.tagName?.toLowerCase() === 'pre' && elementFromNode.querySelector('code') !== null) ||
        elementFromNode.tagName?.toLowerCase() === 'code'
      ) {
        const isBlockCode = elementFromNode.tagName?.toLowerCase() === 'pre' || elementFromNode.innerHTML.match(/\r|\n/) !== null;
        const codeElement = (elementFromNode.tagName?.toLowerCase() === 'pre' ? elementFromNode.querySelector('code') : elementFromNode);
        const snippetLanguage = Array.from(codeElement?.classList ?? []).find(className => className.match('language-'))?.replace('language-', '');
        const codeString = codeElement?.innerHTML ?? '';

        const highlighter = new SyntaxHighlighter({
          codeStringWithMarkup: unescapeHTML(codeString),
          language: snippetLanguage?.trim() !== '' ? snippetLanguage : '',
          hideLanguage: this.props.hideCodeBlockLanguage,
          wrapCodeBlock: this.props.wrapCode,
          unlimitedHeight: this.props.unlimitedCodeBlockHeight,
          codeBlockActions: !isBlockCode
            ? undefined
            : {
                ...Config.getInstance().config.codeBlockActions,
                ...this.props.codeBlockActions
              },
          block: isBlockCode,
          index: isBlockCode ? this.nextCodeBlockIndex : undefined,
          onCopiedToClipboard: this.props.onCopiedToClipboard != null
            ? (type, text, codeBlockIndex) => {
                if (this.props.onCopiedToClipboard != null) {
                  this.props.onCopiedToClipboard(
                    type,
                    text,
                    this.getReferenceTrackerInformationFromElement(highlighter),
                    this.codeBlockStartIndex + (codeBlockIndex ?? 0),
                    this.nextCodeBlockIndex);
                }
              }
            : undefined,
          onCodeBlockAction: this.props.onCodeBlockAction != null
            ? (actionId, data, type, text, refTracker, codeBlockIndex) => {
                this.props.onCodeBlockAction?.(
                  actionId,
                  data,
                  type,
                  text,
                  this.getReferenceTrackerInformationFromElement(highlighter),
                  this.codeBlockStartIndex + (codeBlockIndex ?? 0),
                  this.nextCodeBlockIndex);
              }
            : undefined
        }).render;
        if (this.props.useParts === true) {
          highlighter.classList.add(PARTS_CLASS_NAME);
        }
        if (isBlockCode) {
          ++this.nextCodeBlockIndex;
        }
        return highlighter;
      }

      elementFromNode.childNodes?.forEach((child) => {
        elementFromNode.replaceChild(this.processNode(child as HTMLElement), child);
      });
    }
    return elementFromNode;
  };

  private readonly getReferenceTrackerInformationFromElement = (element: ExtendedHTMLElement | HTMLElement): ReferenceTrackerInformation[] => {
    // cloning the element
    // since we're gonna inject some unique items
    // to get the start indexes
    const codeElement = element.querySelector('code')?.cloneNode(true) as HTMLElement;

    if (codeElement !== undefined) {
      const markerElements = codeElement.querySelectorAll('mark[reference-tracker]');
      if (markerElements.length > 0) {
        return (Array.from(markerElements) as HTMLElement[]).map((mark: HTMLElement, index: number) => {
          // Generating a unique identifier element
          // to get the start index of it inside the code block
          const startIndexText = `__MARK${index}_${generateUID()}_START__`;
          const startIndexTextElement = DomBuilder.getInstance().build({
            type: 'span',
            innerHTML: startIndexText
          });
          // Injecting that unique identifier for the start index inside the current mark element
          mark.insertAdjacentElement('afterbegin', startIndexTextElement);
          // finding that text inside the code element's inner text
          // to get the startIndex
          const startIndex = codeElement.innerText.indexOf(startIndexText);

          // when we get the start index, we need to remove the element
          // to get the next one's start index properly
          // we don't need to calculate the end index because it will be available
          startIndexTextElement.remove();

          // find the original reference tracker information
          const originalRefTrackerInfo = this.props.highlightRangeWithTooltip?.[parseInt(mark.getAttribute('marker-index') ?? '0')];
          return {
            ...originalRefTrackerInfo,
            recommendationContentSpan: {
              start: startIndex,
              end: startIndex + (
                (originalRefTrackerInfo?.recommendationContentSpan?.end ?? 0) -
                (originalRefTrackerInfo?.recommendationContentSpan?.start ?? 0))
            }
          };
        }) as ReferenceTrackerInformation[];
      }
    }

    return [];
  };

  private readonly showHighlightRangeTooltip = (e: MouseEvent, tooltipText: string): void => {
    clearTimeout(this.highlightRangeTooltipTimeout);
    this.highlightRangeTooltipTimeout = setTimeout(() => {
      this.highlightRangeTooltip = new Overlay({
        background: true,
        closeOnOutsideClick: false,
        referenceElement: (e.currentTarget ?? e.target) as HTMLElement,
        removeOtherOverlays: true,
        dimOutside: false,
        verticalDirection: OverlayVerticalDirection.TO_TOP,
        horizontalDirection: OverlayHorizontalDirection.START_TO_RIGHT,
        children: [
          {
            type: 'div',
            classNames: [ 'mynah-ui-syntax-highlighter-highlight-tooltip' ],
            children: [
              new CardBody({
                body: tooltipText,
              }).render
            ]
          }
        ],
      });
    }, PREVIEW_DELAY);
  };

  private readonly hideHighlightRangeTooltip = (): void => {
    clearTimeout(this.highlightRangeTooltipTimeout);
    if (this.highlightRangeTooltip !== null) {
      this.highlightRangeTooltip?.close();
      this.highlightRangeTooltip = null;
    }
  };

  private readonly getContentBodyChildren = (props: CardBodyProps): Array<HTMLElement | ExtendedHTMLElement | DomBuilderObject> => {
    if (props.body != null && props.body.trim() !== '') {
      let incomingBody = props.body;
      if (props.body !== undefined && props.highlightRangeWithTooltip !== undefined && (props.highlightRangeWithTooltip?.length ?? -1) > 0) {
        props.highlightRangeWithTooltip?.forEach((highlightRangeWithTooltip, index) => {
          if (incomingBody !== undefined && highlightRangeWithTooltip.recommendationContentSpan !== undefined) {
            const generatedStartMarkup = `${highlightersWithTooltip.start.markupStart}${highlightersWithTooltip.start.markupAttributes(index.toString())}${highlightersWithTooltip.start.markupEnd}`;
            let calculatedStartIndex = (highlightRangeWithTooltip.recommendationContentSpan.start + (index * (generatedStartMarkup.length + highlightersWithTooltip.end.markup.length)));
            let calculatedEndIndex = (calculatedStartIndex + generatedStartMarkup.length - highlightRangeWithTooltip.recommendationContentSpan.start) + highlightRangeWithTooltip.recommendationContentSpan.end;
            if (calculatedEndIndex > incomingBody.length) {
              calculatedStartIndex = incomingBody.length - 1;
            }
            if (calculatedEndIndex > incomingBody.length) {
              calculatedEndIndex = incomingBody.length - 1;
            }
            incomingBody = incomingBody.slice(0, calculatedStartIndex) + generatedStartMarkup + incomingBody.slice(calculatedStartIndex);
            incomingBody = incomingBody.slice(0, calculatedEndIndex) + highlightersWithTooltip.end.markup + incomingBody.slice(calculatedEndIndex);
          }
        });
      }

      return [
        ...(Array.from(
          DomBuilder.getInstance().build({
            type: 'div',
            innerHTML: `${parseMarkdown(incomingBody, { includeLineBreaks: true })}`,
          }).childNodes
        ).map((node, index) => {
          const processedNode = this.processNode(node as HTMLElement);
          processedNode.setAttribute?.('render-index', index.toString());
          cleanupElement(processedNode);
          return processedNode;
        }))
      ];
    }

    return [];
  };
}
