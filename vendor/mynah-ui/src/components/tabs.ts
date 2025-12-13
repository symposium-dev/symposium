/*!
 * Copyright 2022 Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

// eslint-disable @typescript-eslint/restrict-template-expressions
import { DomBuilder, ExtendedHTMLElement } from '../helper/dom';
import { cancelEvent } from '../helper/events';
import { StyleLoader } from '../helper/style-loader';
import { Button } from './button';
import { Icon, MynahIcons, MynahIconsType } from './icon';
import { Overlay, OverlayHorizontalDirection, OverlayVerticalDirection } from './overlay';

export interface ToggleOption {
  label?: ExtendedHTMLElement | string | HTMLElement;
  icon?: MynahIcons | MynahIconsType | null;
  pinned?: boolean;
  disabled?: boolean;
  selected?: boolean;
  value: string;
  disabledTooltip?: string | ExtendedHTMLElement;
}
interface TabItemRenderProps extends ToggleOption {
  wrapperTestId?: string;
  optionTestId?: string;
  labelTestId?: string;
  closeButtonTestId?: string;
  name: string;
  onChange?: (selectedValue: string) => void;
  onRemove?: (selectedValue: string, domElement: ExtendedHTMLElement) => void;
}
class TabItem {
  render: ExtendedHTMLElement;
  private readonly props: TabItemRenderProps;
  private disabledTooltip?: Overlay;
  private disabledTooltipTimer: ReturnType<typeof setTimeout>;
  constructor (props: TabItemRenderProps) {
    this.props = props;
    this.render = DomBuilder.getInstance().build({
      type: 'span',
      classNames: [ ...(this.props.pinned === true ? [ 'mynah-tab-item-pinned' ] : [ '' ]) ],
      testId: props.wrapperTestId,
      attributes: {
        key: `${this.props.name}-${this.props.value}`,
        title: this.props.label as string ?? '',
      },
      events: {
        ...(this.props.disabled === true && this.props.disabledTooltip !== undefined
          ? {
              mouseenter: () => {
                this.disabledTooltipTimer = setTimeout(() => {
                  this.disabledTooltip = new Overlay({
                    children: [ {
                      type: 'span',
                      classNames: [ 'mynah-tabs-disabled-tooltip-container' ],
                      children: [ this.props.disabledTooltip ?? '' ]
                    } ],
                    closeOnOutsideClick: false,
                    dimOutside: false,
                    referenceElement: this.render,
                    horizontalDirection: OverlayHorizontalDirection.CENTER,
                    verticalDirection: OverlayVerticalDirection.TO_TOP
                  });
                }, 500);
              },
              mouseleave: () => {
                clearTimeout(this.disabledTooltipTimer);
                if (this.disabledTooltip !== undefined) {
                  this.disabledTooltip.close();
                  setTimeout(() => {
                    this.disabledTooltip = undefined;
                  }, 50);
                }
              }
            }
          : {})
      },
      children: [
        {
          type: 'input',
          testId: props.optionTestId,
          classNames: [ 'mynah-tab-item' ],
          attributes: {
            type: 'radio',
            id: `${this.props.name}-${this.props.value}`,
            value: this.props.value,
            name: this.props.name,
            ...(this.props.selected === true ? { checked: 'checked' } : {}),
            ...(this.props.disabled === true ? { disabled: 'disabled' } : {}),
          },
          events: {
            change: () => {
              if (this.props.onChange != null) {
                this.props.onChange(this.props.value);
              }
            }
          },
        },
        {
          type: 'label',
          testId: props.labelTestId,
          classNames: [ 'mynah-tab-item-label' ],
          attributes: {
            for: `${this.props.name}-${this.props.value}`,
          },
          events: {
            dblclick: (e) => {
              cancelEvent(e);
            },
            auxclick: (e) => {
              // only close on middle click
              if (e.button === 1 && this.props.onRemove !== undefined && this.props.pinned !== true) {
                this.props.onRemove(this.props.value, this.render);
              }
            },
          },
          children: [
            this.props.icon != null ? new Icon({ icon: props.icon as MynahIcons }).render : '',
            {
              type: 'span',
              classNames: [ 'mynah-tab-item-label-text' ],
              children: [ this.props.label ?? '' ]
            },
            (this.props.onRemove !== undefined && this.props.pinned !== true)
              ? new Button({
                testId: this.props.closeButtonTestId,
                classNames: [ 'mynah-tabs-close-button' ],
                onClick: () => {
                  if (this.props.onRemove !== undefined) {
                    this.props.onRemove(this.props.value, this.render);
                  }
                },
                icon: new Icon({ icon: MynahIcons.CANCEL }).render,
                primary: false
              }).render
              : ''
          ],
        },
      ],
    });
  }
}
export interface TabProps {
  testId?: string;
  options: ToggleOption[];
  direction?: 'horizontal' | 'vertical';
  value?: string | null;
  name: string;
  disabled?: boolean;
  onChange?: (selectedValue: string) => void;
  onRemove?: (selectedValue: string, domElement: ExtendedHTMLElement) => void;
}
export class Tab {
  render: ExtendedHTMLElement;
  private readonly props: TabProps;
  private currentValue?: string | null;

  constructor (props: TabProps) {
    StyleLoader.getInstance().load('components/_tab.scss');
    this.props = { direction: 'horizontal', ...props };
    this.currentValue = this.props.value;
    this.render = DomBuilder.getInstance().build({
      type: 'div',
      testId: this.props.testId,
      classNames: [ 'mynah-tabs-container', `mynah-tabs-direction-${this.props.direction as string}` ],
      attributes: props.disabled === true ? { disabled: 'disabled' } : {},
      children: this.getChildren(props.value),
      events: {
        wheel: {
          handler: this.transformScroll,
          options: { passive: true }
        }
      }
    });
  }

  private readonly transformScroll = (e: WheelEvent): void => {
    if (e.deltaY === 0) {
      return;
    }
    this.render.scrollLeft += e.deltaY;
  };

  private readonly getChildren = (value?: string | null): any[] => [
    ...this.props.options.map(option => {
      return new TabItem({
        ...option,
        selected: value === option.value,
        name: this.props.name,
        onChange: this.updateSelectionRender,
        onRemove: this.props.onRemove,
        ...(this.props.testId != null
          ? {
              wrapperTestId: `${this.props.testId}-option-wrapper`,
              optionTestId: `${this.props.testId}-option`,
              labelTestId: `${this.props.testId}-option-label`,
              closeButtonTestId: `${this.props.testId}-option-close-button`
            }
          : {})
      }).render;
    })
  ];

  private readonly updateSelectionRender = (value: string): void => {
    if (this.props.onChange !== undefined) {
      this.props.onChange(value);
    }
  };

  setValue = (value: string): void => {
    if (value !== this.getValue()) {
      this.currentValue = value;
      const elmToCheck = this.render.querySelector(`#${this.props.name}-${value}`);
      if (elmToCheck !== undefined) {
        (elmToCheck as HTMLInputElement).click();
        (elmToCheck as HTMLInputElement).checked = true;
        ((elmToCheck as HTMLInputElement).nextSibling as HTMLLabelElement).classList.remove('indication');
      }
    }
  };

  addOption = (option: ToggleOption): void => {
    this.props.options.push(option);
    this.render.appendChild(new TabItem({
      ...option,
      name: this.props.name,
      onChange: this.updateSelectionRender,
      onRemove: this.props.onRemove,
      ...(this.props.testId != null
        ? {
            wrapperTestId: `${this.props.testId}-options-wrapper`,
            optionTestId: `${this.props.testId}-option`,
            labelTestId: `${this.props.testId}-option-label`,
            closeButtonTestId: `${this.props.testId}-option-close-button`
          }
        : {})
    }).render);
    if (option.selected === true) {
      this.setValue(option.value);
      this.snapToOption(option.value);
    }
  };

  removeOption = (value: string): void => {
    this.props.options = this.props.options.filter(option => option.value !== value);
    const elmToCheck = this.render.querySelector(`span[key="${this.props.name}-${value}"]`);
    if (elmToCheck !== undefined) {
      elmToCheck?.remove();
    }
  };

  updateOptionTitle = (value: string, title: string): void => {
    this.props.options = this.props.options.filter(option => option.value !== value);
    const elmToCheck = this.render.querySelector(`span[key="${this.props.name}-${value}"] .mynah-tab-item-label-text`);
    if (elmToCheck !== undefined) {
      (elmToCheck as HTMLSpanElement).innerHTML = title;
    }
  };

  updateOptionIndicator = (value: string, indication: boolean): void => {
    this.props.options = this.props.options.filter(option => option.value !== value);
    const elmToCheck: HTMLLabelElement | null = this.render.querySelector(`label[for="${this.props.name}-${value}"]`);
    if (elmToCheck !== null) {
      if (indication && value !== this.getValue()) {
        elmToCheck.classList.add('indication');
      } else {
        elmToCheck.classList.remove('indication');
      }
    }
  };

  snapToOption = (value: string): void => {
    const elmToCheck = this.render.querySelector(`#${this.props.name}-${value}`);
    if (elmToCheck !== undefined) {
      this.render.scrollLeft = (elmToCheck?.parentNode as HTMLElement).offsetLeft;
    }
  };

  getValue = (): string | undefined | null => this.currentValue;
}
