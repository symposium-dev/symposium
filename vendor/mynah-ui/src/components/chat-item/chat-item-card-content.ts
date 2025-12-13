/*!
 * Copyright 2022 Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

import { DomBuilderObject, ExtendedHTMLElement, getTypewriterPartsCss } from '../../helper/dom';
import { CardRenderDetails, ChatItem, CodeBlockActions, OnCodeBlockActionFunction, OnCopiedToClipboardFunction, ReferenceTrackerInformation } from '../../static';
import { CardBody } from '../card/card-body';
import { generateUID } from '../../helper/guid';
import { Config } from '../../helper/config';
export interface ChatItemCardContentProps {
  body?: string | null;
  testId?: string;
  renderAsStream?: boolean;
  classNames?: string[];
  unlimitedCodeBlockHeight?: boolean;
  hideCodeBlockLanguage?: boolean;
  wrapCode?: boolean;
  codeReference?: ReferenceTrackerInformation[] | null;
  onAnimationStateChange?: (isAnimating: boolean) => void;
  contentProperties?: {
    codeBlockActions?: CodeBlockActions;
    onLinkClick?: (url: string, e: MouseEvent) => void;
    onCopiedToClipboard?: OnCopiedToClipboardFunction;
    onCodeBlockAction?: OnCodeBlockActionFunction;
  };
  children?: Array<ExtendedHTMLElement | HTMLElement | string | DomBuilderObject>;
}
export class ChatItemCardContent {
  private props: ChatItemCardContentProps;
  render: ExtendedHTMLElement;
  contentBody: CardBody | null = null;
  private readonly updateStack: Array<Partial<ChatItem>> = [];
  private typewriterItemIndex: number = 0;
  private readonly typewriterId: string = `typewriter-card-${generateUID()}`;
  private lastAnimationDuration: number = 0;
  private updateTimer: ReturnType<typeof setTimeout> | undefined;
  private isStreamActive: boolean = true;
  constructor (props: ChatItemCardContentProps) {
    this.props = props;
    this.contentBody = this.getCardContent();
    this.render = this.contentBody.render;

    if ((this.props.renderAsStream ?? false) && (this.props.body ?? '').trim() !== '') {
      this.updateCardStack({});
    }
  }

  private readonly getCardContent = (): CardBody => {
    return new CardBody({
      body: this.props.body ?? '',
      hideCodeBlockLanguage: this.props.hideCodeBlockLanguage,
      wrapCode: this.props.wrapCode,
      unlimitedCodeBlockHeight: this.props.unlimitedCodeBlockHeight,
      testId: this.props.testId,
      useParts: this.props.renderAsStream,
      classNames: [ this.typewriterId, ...(this.props.classNames ?? []) ],
      highlightRangeWithTooltip: this.props.codeReference,
      children: this.props.children,
      ...this.props.contentProperties,
    });
  };

  private readonly updateCard = (): void => {
    if (this.updateTimer === undefined && this.updateStack.length > 0) {
      const updateWith: Partial<ChatItemCardContentProps> | undefined = this.updateStack.shift();
      if (updateWith !== undefined) {
        this.props = {
          ...this.props,
          ...updateWith,
        };

        const newCardContent = this.getCardContent();
        const upcomingWords = Array.from(newCardContent.render.querySelectorAll('.typewriter-part') ?? []);
        for (let i = 0; i < upcomingWords.length; i++) {
          upcomingWords[i].setAttribute('index', i.toString());
        }
        // How many new words will be added
        const newWordsCount = upcomingWords.length - this.typewriterItemIndex;

        // For each stack, without exceeding 500ms in total
        // we're setting each words delay time according to the count of them.
        const stackTime = Config.getInstance().config.typewriterStackTime ?? 100;
        const maxWordTime = Config.getInstance().config.typewriterMaxWordTime ?? 20;
        const disableAnimation = Config.getInstance().config.disableTypewriterAnimation ?? false;
        const shouldAnimate = !disableAnimation && this.isStreamActive;
        const timeForEach = shouldAnimate ? Math.min(maxWordTime, Math.floor(stackTime / newWordsCount)) : 0;

        // Generate animator style and inject into render
        // CSS animations ~100 times faster then js timeouts/intervals
        if (shouldAnimate) {
          newCardContent.render.insertAdjacentElement('beforeend',
            getTypewriterPartsCss(this.typewriterId, this.typewriterItemIndex, upcomingWords.length, timeForEach));
        }

        this.props.onAnimationStateChange?.(shouldAnimate);
        if (this.contentBody == null) {
          this.contentBody = newCardContent;
          this.render = this.contentBody.render;
        }
        this.updateDOMContent(newCardContent);
        this.lastAnimationDuration = shouldAnimate ? timeForEach * newWordsCount : 0;

        // If there is another set
        // call the same function to check after current stack totally shown
        this.updateTimer = setTimeout(() => {
          this.updateTimer = undefined;
          // Only signal animation end if no more updates are queued
          this.props.onAnimationStateChange?.(this.updateStack.length > 0);
          this.updateCard();
        }, this.lastAnimationDuration);
      }
    }
  };

  public readonly updateCardStack = (updateWith: Partial<ChatItemCardContentProps>): void => {
    this.updateStack.push(updateWith);
    this.updateCard();
  };

  public readonly endStream = (): void => {
    this.isStreamActive = false;
    this.flushRemainingUpdates();
  };

  private readonly updateDOMContent = (newCardContent: CardBody): void => {
    const upcomingWords = Array.from(newCardContent.render.querySelectorAll('.typewriter-part') ?? []);
    for (let i = 0; i < upcomingWords.length; i++) {
      upcomingWords[i].setAttribute('index', i.toString());
    }

    if (this.contentBody == null) {
      this.contentBody = newCardContent;
      this.render = this.contentBody.render;
    }
    Array.from(newCardContent.render.childNodes).forEach(node => {
      const newElm = node as HTMLElement;
      const currIndex = (node as HTMLElement).getAttribute('render-index');
      const oldElm = this.render.querySelector(`[render-index="${currIndex ?? ''}"]`);
      if (oldElm == null) {
        this.render.insertChild('beforeend', node as HTMLElement);
      } else if (newElm.innerHTML !== oldElm.innerHTML) {
        if (newElm.classList.contains('mynah-syntax-highlighter')) {
          const newPreElm = newElm.querySelector('pre');
          if (newPreElm?.childNodes != null) {
            const oldElmPre = oldElm.querySelector('pre');
            if (oldElmPre != null) {
              oldElmPre.replaceChildren(...Array.from(newPreElm.childNodes));
              if (!newElm.classList.contains('mynah-inline-code') && !newElm.classList.contains('no-max') && oldElmPre.scrollHeight > oldElmPre.clientHeight) {
                oldElm.classList.add('max-height-exceed');
              }
            }
          }
        } else {
          oldElm.replaceWith(newElm);
        }
      }
    });
    this.contentBody = newCardContent;
    this.typewriterItemIndex = upcomingWords.length;
  };

  private readonly flushRemainingUpdates = (): void => {
    if (this.updateTimer != null) {
      clearTimeout(this.updateTimer);
      this.updateTimer = undefined;
    }

    // Clean up all animation styles
    const existingAnimationStyles = this.render.querySelectorAll(`style[data-typewriter="${this.typewriterId}"], style[type="text/css"]`);
    existingAnimationStyles.forEach(style => {
      if (style.innerHTML.includes(this.typewriterId)) {
        style.remove();
      }
    });

    // Batch all updates into final props
    if (this.updateStack.length > 0) {
      const finalProps = this.updateStack.reduce((acc, update) => ({ ...acc, ...update }), this.props);
      this.updateStack.length = 0; // Clear array efficiently

      this.props = finalProps;
      const newCardContent = this.getCardContent();
      this.updateDOMContent(newCardContent);
    }

    this.props.onAnimationStateChange?.(false);
  };

  public readonly getRenderDetails = (): CardRenderDetails => {
    return {
      totalNumberOfCodeBlocks: (this.contentBody?.nextCodeBlockIndex ?? 0)
    };
  };
}
