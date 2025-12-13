/*!
 * Copyright 2022 Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

/* eslint-disable @typescript-eslint/brace-style */
import { DomBuilder, DomBuilderObject, ExtendedHTMLElement } from '../helper/dom';
import { generateUID } from '../helper/guid';
import { StyleLoader } from '../helper/style-loader';
import { MynahPortalNames } from '../static';

export const OVERLAY_MARGIN = 8;
/**
 * The horizontal creation direction of the overlay
 */
export enum OverlayHorizontalDirection {
  /**
     * starts from the left edge of the reference element and opens to left
     */
  TO_LEFT = 'horizontal-direction-to-left',
  /**
     * starts from the right edge of the reference element and opens to left
     */
  END_TO_LEFT = 'horizontal-direction-from-end-to-left',
  /**
     * starts from the right edge of the reference element and opens to right
     */
  TO_RIGHT = 'horizontal-direction-to-right',
  /**
     * starts from the left edge of the reference element and opens to right
     */
  START_TO_RIGHT = 'horizontal-direction-from-start-to-right',
  /**
     * starts and opens at the center of the reference element
     */
  CENTER = 'horizontal-direction-at-center',
}

/**
 * The vertical creation direction of the overlay
 */
export enum OverlayVerticalDirection {
  /**
     * starts from the bottom edge of the reference element and opens to bottom
     */
  TO_BOTTOM = 'vertical-direction-to-bottom',
  /**
     * starts from the top edge of the reference element and opens to bottom
     */
  START_TO_BOTTOM = 'vertical-direction-from-start-to-bottom',
  /**
     * starts from the top edge of the reference element and opens to top
     */
  TO_TOP = 'vertical-direction-to-top',
  /**
     * starts from the bottom edge of the reference element and opens to top
     */
  END_TO_TOP = 'vertical-direction-from-end-to-top',
  /**
     * starts and opens at the center of the reference element
     */
  CENTER = 'vertical-direction-at-center',
}

export interface OverlayProps {
  testId?: string;
  referenceElement?: HTMLElement | ExtendedHTMLElement;
  removeIfReferenceElementRemoved?: boolean;
  referencePoint?: { top: number; left: number };
  children: Array<HTMLElement | ExtendedHTMLElement | DomBuilderObject>;
  horizontalDirection?: OverlayHorizontalDirection;
  verticalDirection?: OverlayVerticalDirection;
  stretchWidth?: boolean;
  dimOutside?: boolean;
  closeOnOutsideClick?: boolean;
  background?: boolean;
  onClose?: () => void;
  removeOtherOverlays?: boolean;
}
export class Overlay {
  render: ExtendedHTMLElement;
  private readonly container: ExtendedHTMLElement;
  private readonly innerContainer: ExtendedHTMLElement;
  private readonly guid = generateUID();
  private readonly onClose;

  constructor (props: OverlayProps) {
    StyleLoader.getInstance().load('components/_overlay.scss');
    const horizontalDirection = props.horizontalDirection ?? OverlayHorizontalDirection.TO_RIGHT;
    const verticalDirection = props.verticalDirection ?? OverlayVerticalDirection.START_TO_BOTTOM;
    this.onClose = props.onClose;
    const dimOutside = props.dimOutside !== false;
    const closeOnOutsideClick = props.closeOnOutsideClick !== false;

    const calculatedTop = this.getCalculatedTop(verticalDirection, props.referenceElement, props.referencePoint);
    const calculatedLeft = this.getCalculatedLeft(horizontalDirection, props.referenceElement, props.referencePoint);
    const calculatedWidth = props.stretchWidth === true ? this.getCalculatedWidth(props.referenceElement) : 0;

    this.innerContainer = DomBuilder.getInstance().build({
      type: 'div',
      classNames: [ 'mynah-overlay-inner-container' ],
      children: props.children,
    });

    this.container = DomBuilder.getInstance().build({
      type: 'div',
      classNames: [ 'mynah-overlay-container', horizontalDirection, verticalDirection, props.background !== false ? 'background' : '' ],
      attributes: {
        style: `top: ${calculatedTop}px; left: ${calculatedLeft}px; ${calculatedWidth !== 0 ? `width: ${calculatedWidth}px;` : ''}`,
      },
      children: [ this.innerContainer ],
    });

    if (props.removeOtherOverlays === true) {
      DomBuilder.getInstance().removeAllPortals(MynahPortalNames.OVERLAY);
    }

    // this is a portal that goes over all the other items
    // to make it as an overlay item
    this.render = DomBuilder.getInstance().createPortal(
      `${MynahPortalNames.OVERLAY}-${this.guid}`,
      {
        type: 'div',
        testId: props.testId,
        attributes: { id: `mynah-overlay-${this.guid}` },
        classNames: [
          'mynah-overlay',
          ...(dimOutside ? [ 'mynah-overlay-dim-outside' ] : []),
          ...(closeOnOutsideClick ? [ 'mynah-overlay-close-on-outside-click' ] : []),
        ],
        events: {
          click: closeOnOutsideClick
            ? (event: MouseEvent) => {
                // Only close if the click is outside the overlay
                if (event.target === event.currentTarget) {
                  this.close();
                }
              }
            : () => {},
        },
        children: [ this.container ],
      },
      'beforeend'
    );

    // Screen edge fixes
    const winHeight = Math.max(document.documentElement.clientHeight ?? 0, window.innerHeight ?? 0);
    const winWidth = Math.max(document.documentElement.clientWidth ?? 0, window.innerWidth ?? 0);
    const lastContainerRect = this.container.getBoundingClientRect();
    const effectiveTop = parseFloat(this.container.style.top ?? '0');
    const effectiveLeft = parseFloat(this.container.style.left ?? '0');

    // Vertical edge
    // Check top exceeding
    if (lastContainerRect.top < OVERLAY_MARGIN) {
      this.container.style.top = `${effectiveTop + (OVERLAY_MARGIN - lastContainerRect.top)}px`;
    } // Check bottom exceeding
    else if (lastContainerRect.top + lastContainerRect.height + OVERLAY_MARGIN > winHeight) {
      this.container.style.top = `${effectiveTop - (lastContainerRect.top + lastContainerRect.height + OVERLAY_MARGIN - winHeight)}px`;
    }

    // Horizontal edge
    // Check left exceeding
    if (lastContainerRect.left < OVERLAY_MARGIN) {
      this.container.style.left = `${effectiveLeft + (OVERLAY_MARGIN - lastContainerRect.left)}px`;
    } // Check right exceeding
    else if (lastContainerRect.left + lastContainerRect.width + OVERLAY_MARGIN > winWidth) {
      this.container.style.left = `${effectiveLeft - (lastContainerRect.left + lastContainerRect.width + OVERLAY_MARGIN - winWidth)}px`;
    }

    this.preventTransformBlur();

    // Check if reference element is still on dom tree
    if (MutationObserver != null && props.removeIfReferenceElementRemoved !== false && props.referenceElement != null) {
      const observer = new MutationObserver(() => {
        if (!document.contains(props.referenceElement as HTMLElement)) {
          this.close();
          observer.disconnect();
        }
      });

      // Observe the document body for any subtree modifications
      observer.observe(document.body, {
        childList: true,
        subtree: true
      });
    }

    // we need to delay the class toggle
    // to avoid the skipping of the transition comes from css
    // for a known js-css relation problem
    setTimeout(() => {
      this.render.addClass('mynah-overlay-open');

      if (closeOnOutsideClick) {
        window.addEventListener('blur', this.windowBlurHandler.bind(this));
        window.addEventListener('resize', this.windowBlurHandler.bind(this));
      }
    }, 10);
  }

  /**
   * Applying a transform with a fractional pixel value causes bluriness on certain displays.
   *
   * Since transform uses --overlayTopPos which is a percentage of the overlay's height, and the height can be a fractional
   * pixel value if line-height is fractional, this function rounds --overlayTopPos to an integer pixel value to prevent bluriness.
   */
  private readonly preventTransformBlur = (): void => {
    if (ResizeObserver != null) {
      const observer = new ResizeObserver(() => {
        const lastContainerRect = this.container.getBoundingClientRect();
        const height = lastContainerRect.height;

        const style = getComputedStyle(this.container);
        const shiftPercent = parseFloat(style.getPropertyValue('--overlayTopPos'));

        const shiftPixels = Math.round(height * shiftPercent / 100);

        this.container.style.transform = `translate3d(var(--overlayLeftPos), ${shiftPixels}px, 0)`;
      });

      observer.observe(this.container);
    }
  };

  close = (): void => {
    this.render.removeClass('mynah-overlay-open');
    // In this timeout, we're waiting the close animation to be ended
    setTimeout(() => {
      this.render.remove();
    }, 250);
    if (this.onClose !== undefined) {
      this.onClose();
    }
  };

  private readonly windowBlurHandler = (): void => {
    this.close();
    window.removeEventListener('blur', this.windowBlurHandler.bind(this));
    window.removeEventListener('resize', this.windowBlurHandler.bind(this));
  };

  private readonly getCalculatedLeft = (
    horizontalDirection: OverlayHorizontalDirection,
    referenceElement?: HTMLElement | ExtendedHTMLElement,
    referencePoint?: { top?: number; left: number }
  ): number => {
    const referenceRectangle =
            referenceElement !== undefined
              ? referenceElement.getBoundingClientRect()
              : referencePoint !== undefined
                ? { left: referencePoint.left, width: 0 }
                : { left: 0, width: 0 };

    switch (horizontalDirection.toString()) {
      case OverlayHorizontalDirection.TO_RIGHT:
        return referenceRectangle.left + referenceRectangle.width + OVERLAY_MARGIN;
      case OverlayHorizontalDirection.START_TO_RIGHT:
        return referenceRectangle.left;
      case OverlayHorizontalDirection.TO_LEFT:
        return referenceRectangle.left - OVERLAY_MARGIN;
      case OverlayHorizontalDirection.END_TO_LEFT:
        return referenceRectangle.left + referenceRectangle.width;
      case OverlayHorizontalDirection.CENTER:
        return referenceRectangle.left + referenceRectangle.width / 2;
      default:
        return 0;
    }
  };

  private readonly getCalculatedWidth = (
    referenceElement?: HTMLElement | ExtendedHTMLElement
  ): number => {
    return referenceElement !== undefined
      ? referenceElement.getBoundingClientRect().width
      : 0;
  };

  private readonly getCalculatedTop = (
    verticalDirection: OverlayVerticalDirection,
    referenceElement?: HTMLElement | ExtendedHTMLElement,
    referencePoint?: { top: number; left?: number }
  ): number => {
    const referenceRectangle =
            referenceElement !== undefined
              ? referenceElement.getBoundingClientRect()
              : referencePoint !== undefined
                ? { top: referencePoint.top, height: 0 }
                : { top: 0, height: 0 };

    switch (verticalDirection.toString()) {
      case OverlayVerticalDirection.TO_BOTTOM:
        return referenceRectangle.top + referenceRectangle.height + OVERLAY_MARGIN;
      case OverlayVerticalDirection.START_TO_BOTTOM:
        return referenceRectangle.top;
      case OverlayVerticalDirection.TO_TOP:
        return referenceRectangle.top - OVERLAY_MARGIN;
      case OverlayVerticalDirection.END_TO_TOP:
        return referenceRectangle.top + referenceRectangle.height;
      case OverlayVerticalDirection.CENTER:
        return referenceRectangle.top + referenceRectangle.height / 2;
      default:
        return referenceRectangle.top;
    }
  };

  public updateContent = (children: Array<string | DomBuilderObject | HTMLElement | ExtendedHTMLElement>): void => {
    this.innerContainer.update({ children });
  };

  public toggleHidden = (hidden: boolean): void => {
    this.render.hidden = hidden;
  };
}
