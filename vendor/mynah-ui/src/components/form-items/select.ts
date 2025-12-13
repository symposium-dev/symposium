/*!
 * Copyright 2022 Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

import { Config } from '../../helper/config';
import { DomBuilder, DomBuilderObject, ExtendedHTMLElement } from '../../helper/dom';
import { StyleLoader } from '../../helper/style-loader';
import { Icon, MynahIcons, MynahIconsType } from '../icon';
import { Card } from '../card/card';
import { CardBody } from '../card/card-body';
import { Overlay, OverlayHorizontalDirection, OverlayVerticalDirection } from '../overlay';

const TOOLTIP_DELAY = 350;

interface SelectOption {
  value: string;
  label: string;
  description?: string;
}

export interface SelectProps {
  classNames?: string[];
  attributes?: Record<string, string>;
  handleIcon?: MynahIcons | MynahIconsType;
  border?: boolean;
  icon?: MynahIcons | MynahIconsType;
  label?: HTMLElement | ExtendedHTMLElement | string;
  description?: ExtendedHTMLElement;
  value?: string;
  optional?: boolean;
  autoWidth?: boolean;
  options?: SelectOption[];
  placeholder?: string;
  onChange?: (value: string) => void;
  wrapperTestId?: string;
  optionTestId?: string;
  tooltip?: string;
}

export abstract class SelectAbstract {
  render: ExtendedHTMLElement;
  setValue = (value: string): void => {};
  getValue = (): string => '';
  setEnabled = (enabled: boolean): void => {};
}

export class SelectInternal {
  private readonly props: SelectProps;
  private readonly selectElement: ExtendedHTMLElement;
  private readonly autoWidthSizer: ExtendedHTMLElement;
  private readonly selectContainer: ExtendedHTMLElement;
  private tooltipOverlay: Overlay | null = null;
  private tooltipTimeout: ReturnType<typeof setTimeout> | null = null;
  render: ExtendedHTMLElement;
  constructor (props: SelectProps) {
    this.props = props;
    StyleLoader.getInstance().load('components/_form-input.scss');
    this.autoWidthSizer = DomBuilder.getInstance().build({
      type: 'span',
      classNames: [ 'select-auto-width-sizer' ],
      children: [ this.props.options?.find(option => option.value === this.props.value)?.label ?? this.props.placeholder ?? '' ]
    });
    this.selectElement = DomBuilder.getInstance().build({
      type: 'select',
      testId: props.wrapperTestId,
      classNames: [ 'mynah-form-input', ...(props.classNames ?? []), ...(props.autoWidth === true ? [ 'auto-width' ] : []) ],
      events: {
        change: (e) => {
          const value = (e.currentTarget as HTMLSelectElement).value;
          if (props.onChange !== undefined) {
            props.onChange(value);
          }
          this.autoWidthSizer.update({
            children: [ this.props.options?.find(option => option.value === value)?.label ?? this.props.placeholder ?? '' ]
          });
        }
      },
      children:
        [ ...(props.optional === true
          ? [ {
              label: props.placeholder ?? '...',
              value: '',
              description: undefined
            } ]
          : []), ...props.options ?? [] ].map(option => ({
          type: 'option',
          testId: props.optionTestId,
          classNames: option.value === '' ? [ 'empty-option' ] : [],
          attributes: {
            value: option.value,
          },
          children: [
            option.label
          ]
        })) as DomBuilderObject[]
    });
    if (props.value !== undefined) {
      this.selectElement.value = props.value;
    }
    this.selectContainer = DomBuilder.getInstance().build({
      type: 'div',
      classNames: [ 'mynah-form-input-container', ...(props.border === false ? [ 'no-border' ] : []) ],
      ...(props.attributes !== undefined ? { attributes: props.attributes } : {}),
      children: [
        ...(props.icon
          ? [ new Icon({ icon: props.icon, classNames: [ 'mynah-form-input-icon' ] }).render ]
          : []),
        ...(props.autoWidth !== undefined ? [ this.autoWidthSizer ] : []),
        this.selectElement,
        new Icon({ icon: props.handleIcon ?? MynahIcons.DOWN_OPEN, classNames: [ 'mynah-select-handle' ] }).render ]
    });

    this.render = DomBuilder.getInstance().build({
      type: 'div',
      classNames: [ 'mynah-form-input-wrapper' ],
      children: [
        {
          type: 'span',
          classNames: [ 'mynah-form-input-label' ],
          children: [ ...(props.label !== undefined ? [ props.label ] : []) ]
        },
        ...[ props.description !== undefined ? props.description : '' ],
        this.selectContainer
      ]
    });

    // Add tooltip functionality
    this.setupTooltip();
  }

  private readonly setupTooltip = (): void => {
    this.selectContainer.update({
      events: {
        mouseenter: () => {
          const currentTooltip = this.getCurrentTooltip();
          if (currentTooltip != null && currentTooltip.trim() !== '') {
            this.showTooltip(currentTooltip);
          }
        },
        mouseleave: () => {
          this.hideTooltip();
        }
      }
    });
  };

  private readonly getCurrentTooltip = (): string => {
    const currentValue = this.selectElement.value;
    const selectedOption = this.props.options?.find(option => option.value === currentValue);

    // If there's a selected option, show label and description; otherwise use the base tooltip
    if (selectedOption?.description != null) {
      return `<strong>${selectedOption.label}</strong><br>${selectedOption.description}`;
    }
    return this.props.tooltip ?? '';
  };

  setValue = (value: string): void => {
    this.selectElement.value = value;
    this.autoWidthSizer.update({
      children: [ this.props.options?.find(option => option.value === value)?.label ?? this.props.placeholder ?? '' ]
    });
  };

  getValue = (): string => {
    return this.selectElement.value;
  };

  setEnabled = (enabled: boolean): void => {
    if (enabled) {
      this.selectElement.removeAttribute('disabled');
    } else {
      this.selectElement.setAttribute('disabled', 'disabled');
    }
  };

  private readonly showTooltip = (content: string): void => {
    if (content.trim() !== '') {
      // Clear any existing timeout
      if (this.tooltipTimeout !== null) {
        clearTimeout(this.tooltipTimeout);
      }

      this.tooltipTimeout = setTimeout(() => {
        this.tooltipOverlay = new Overlay({
          background: true,
          closeOnOutsideClick: false,
          referenceElement: this.selectContainer,
          dimOutside: false,
          removeOtherOverlays: true,
          verticalDirection: OverlayVerticalDirection.TO_TOP,
          horizontalDirection: OverlayHorizontalDirection.START_TO_RIGHT,
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

  private readonly hideTooltip = (): void => {
    // Clear any pending timeout
    if (this.tooltipTimeout !== null) {
      clearTimeout(this.tooltipTimeout);
      this.tooltipTimeout = null;
    }

    // Close existing tooltip
    if (this.tooltipOverlay !== null) {
      this.tooltipOverlay.close();
      this.tooltipOverlay = null;
    }
  };
}

export class Select extends SelectAbstract {
  render: ExtendedHTMLElement;

  constructor (props: SelectProps) {
    super();
    return new (Config.getInstance().config.componentClasses.Select ?? SelectInternal)(props);
  }

  setValue = (value: string): void => {};
  getValue = (): string => '';
  setEnabled = (enabled: boolean): void => {};
}
