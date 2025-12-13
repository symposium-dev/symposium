/*!
 * Copyright 2022 Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

import {
  DomBuilder,
  DomBuilderEventHandler,
  DomBuilderEventHandlerWithOptions,
  DomBuilderObject,
  ExtendedHTMLElement,
  GenericEvents
} from '../helper/dom';
import { Overlay, OverlayHorizontalDirection, OverlayVerticalDirection } from './overlay';
import { Card } from './card/card';
import { CardBody } from './card/card-body';
import { Config } from '../helper/config';
import { cancelEvent } from '../helper/events';
import escapeHTML from 'escape-html';
import unescapeHTML from 'unescape-html';
import { parseMarkdown } from '../helper/marked';
import { StyleLoader } from '../helper/style-loader';
import { Icon } from './icon';

const TOOLTIP_DELAY = 350;
export interface ButtonProps {
  classNames?: string[];
  attributes?: Record<string, string>;
  icon?: HTMLElement | ExtendedHTMLElement;
  testId?: string;
  label?: HTMLElement | ExtendedHTMLElement | string;
  confirmation?: {
    confirmButtonText: string;
    cancelButtonText: string;
    title: string;
    description?: string;
  };
  tooltip?: string;
  tooltipVerticalDirection?: OverlayVerticalDirection;
  tooltipHorizontalDirection?: OverlayHorizontalDirection;
  children?: Array<HTMLElement | ExtendedHTMLElement | string>;
  disabled?: boolean;
  hidden?: boolean;
  primary?: boolean;
  border?: boolean;
  status?: 'main' | 'primary' | 'info' | 'success' | 'warning' | 'error' | 'clear' | 'dimmed-clear';
  fillState?: 'hover' | 'always';
  additionalEvents?: Partial<Record<GenericEvents, DomBuilderEventHandler | DomBuilderEventHandlerWithOptions>>;
  onClick: (e: Event) => void;
  onHover?: (e: Event) => void;
}
export abstract class ButtonAbstract {
  render: ExtendedHTMLElement;
  updateLabel = (label: HTMLElement | ExtendedHTMLElement | string): void => {
  };

  setHidden = (hidden: boolean): void => {
  };

  setEnabled = (enabled: boolean): void => {
  };

  hideTooltip = (): void => {
  };
}

class ButtonInternal extends ButtonAbstract {
  render: ExtendedHTMLElement;
  private readonly props: ButtonProps;
  private tooltipOverlay: Overlay | null;
  private tooltipTimeout: ReturnType<typeof setTimeout>;
  constructor (props: ButtonProps) {
    StyleLoader.getInstance().load('components/_button.scss');
    super();
    this.props = props;
    this.render = DomBuilder.getInstance().build({
      type: 'button',
      classNames: [
        'mynah-button',
        ...(props.primary === false ? [ 'mynah-button-secondary' ] : []),
        ...(props.border === true ? [ 'mynah-button-border' ] : []),
        ...(props.hidden === true ? [ 'hidden' ] : []),
        ...([ `fill-state-${props.fillState ?? 'always'}` ]),
        ...(props.status != null ? [ `status-${props.status}` ] : []),
        ...(props.classNames !== undefined ? props.classNames : []),
      ],
      testId: props.testId,
      attributes: {
        ...(props.disabled === true ? { disabled: 'disabled' } : {}),
        tabindex: '0',
        ...props.attributes,
      },
      events: {
        ...props.additionalEvents,
        click: (e) => {
          this.hideTooltip();
          cancelEvent(e);
          if (this.props.disabled !== true) {
            if (this.props.confirmation != null) {
              const confirmationOverlay = new Overlay({
                onClose: () => {
                },
                children: [
                  {
                    type: 'div',
                    classNames: [ 'mynah-button-confirmation-dialog-container' ],
                    children: [
                      {
                        type: 'div',
                        classNames: [ 'mynah-button-confirmation-dialog-header' ],
                        children: [
                          new Icon({ icon: 'warning' }).render,
                          {
                            type: 'h4',
                            children: [ this.props.confirmation.title ]
                          },
                          new Button({
                            icon: new Icon({ icon: 'cancel' }).render,
                            onClick: () => {
                              confirmationOverlay.close();
                            },
                            primary: false,
                            status: 'clear',
                          }).render,
                        ]
                      },
                      {
                        type: 'div',
                        classNames: [ 'mynah-button-confirmation-dialog-body' ],
                        innerHTML: parseMarkdown(this.props.confirmation.description ?? '')
                      },
                      {
                        type: 'div',
                        classNames: [ 'mynah-button-confirmation-dialog-buttons' ],
                        children: [
                          new Button({
                            label: this.props.confirmation.cancelButtonText,
                            onClick: () => {
                              confirmationOverlay.close();
                            },
                            primary: false,
                            status: 'clear',
                          }).render,
                          new Button({
                            label: this.props.confirmation.confirmButtonText,
                            onClick: () => {
                              confirmationOverlay.close();
                              props.onClick(e);
                            },
                            primary: true,
                          }).render
                        ]
                      }
                    ]
                  }
                ],
                background: true,
                closeOnOutsideClick: false,
                dimOutside: true,
                horizontalDirection: OverlayHorizontalDirection.CENTER,
                verticalDirection: OverlayVerticalDirection.CENTER,
                referencePoint: { top: window.innerHeight / 2, left: window.innerWidth / 2 }
              });
            } else {
              props.onClick(e);
            }
          }
        },
        mouseover: (e) => {
          cancelEvent(e);
          if (this.props.onHover != null) {
            this.props.onHover(e);
          }
          const textContentSpan: HTMLSpanElement | null = this.render.querySelector('.mynah-button-label');
          let tooltipText;
          if (props.label != null && typeof props.label === 'string' && textContentSpan != null && textContentSpan.offsetWidth < textContentSpan.scrollWidth) {
            tooltipText = parseMarkdown(props.label ?? '', { includeLineBreaks: true });
          }
          if (props.tooltip !== undefined) {
            if (tooltipText != null) {
              tooltipText += '\n\n';
            } else {
              tooltipText = '';
            }
            tooltipText += parseMarkdown(props.tooltip ?? '', { includeLineBreaks: true });
          }
          if (tooltipText != null) {
            this.showTooltip(tooltipText);
          }
        },
        mouseleave: this.hideTooltip
      },
      children: [
        ...(props.icon !== undefined ? [ props.icon ] : []),
        ...(this.getButtonLabelDomBuilderObject(props.label)),
        ...(props.children ?? []),
      ],
    });
  }

  private readonly getButtonLabelDomBuilderObject = (label?: HTMLElement | ExtendedHTMLElement | string): DomBuilderObject[] => {
    if (label !== undefined) {
      if (typeof label !== 'string') {
        return [ { type: 'span', classNames: [ 'mynah-button-label' ], children: [ label ] } ];
      } else {
        return [ { type: 'span', classNames: [ 'mynah-button-label' ], innerHTML: parseMarkdown(unescapeHTML(escapeHTML(label)), { inline: true }) } ];
      }
    }
    return [];
  };

  private readonly showTooltip = (content: string): void => {
    if (content.trim() !== undefined) {
      clearTimeout(this.tooltipTimeout);
      this.tooltipTimeout = setTimeout(() => {
        const elm: HTMLElement = this.render;
        this.tooltipOverlay = new Overlay({
          background: true,
          closeOnOutsideClick: false,
          referenceElement: elm,
          dimOutside: false,
          removeOtherOverlays: true,
          verticalDirection: this.props.tooltipVerticalDirection ?? OverlayVerticalDirection.TO_TOP,
          horizontalDirection: this.props.tooltipHorizontalDirection ?? OverlayHorizontalDirection.START_TO_RIGHT,
          children: [
            new Card({
              border: false,
              children: [
                new CardBody({
                  body: content
                }).render
              ]
            }).render
          ],
        });
      }, TOOLTIP_DELAY);
    }
  };

  public readonly hideTooltip = (): void => {
    clearTimeout(this.tooltipTimeout);
    if (this.tooltipOverlay !== null) {
      this.tooltipOverlay?.close();
      this.tooltipOverlay = null;
    }
  };

  public readonly updateLabel = (label: HTMLElement | ExtendedHTMLElement | string): void => {
    (this.render.querySelector('.mynah-button-label') as ExtendedHTMLElement).replaceWith(
      DomBuilder.getInstance().build(this.getButtonLabelDomBuilderObject(label)[0])
    );
  };

  public readonly setEnabled = (enabled: boolean): void => {
    this.props.disabled = !enabled;
    if (enabled) {
      this.render.removeAttribute('disabled');
    } else {
      this.render.setAttribute('disabled', 'disabled');
    }
  };

  public readonly setHidden = (hidden: boolean): void => {
    this.props.hidden = hidden;
    if (hidden) {
      this.render.classList.add('hidden');
    } else {
      this.render.classList.remove('hidden');
    }
  };
}

export class Button extends ButtonAbstract {
  render: ExtendedHTMLElement;

  constructor (props: ButtonProps) {
    super();
    return (new (Config.getInstance().config.componentClasses.Button ?? ButtonInternal)(props));
  }

  updateLabel = (label: HTMLElement | ExtendedHTMLElement | string): void => {
  };

  setEnabled = (enabled: boolean): void => {
  };

  setHidden = (hidden: boolean): void => {
  };

  hideTooltip = (): void => {
  };
}
