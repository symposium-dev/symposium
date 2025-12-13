/*!
 * Copyright 2022 Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

import { DomBuilder, ExtendedHTMLElement } from '../../helper/dom';
import { Button } from '../button';
import { Icon, MynahIcons } from '../icon';
import { generateUID } from '../../helper/guid';
import { MynahUIGlobalEvents } from '../../helper/events';
import { MynahEventNames, MynahPortalNames } from '../../static';
import testIds from '../../helper/test-ids';

export interface BaseDropdownProps<T> {
  description?: string;
  descriptionLink?: {
    id: string;
    text: string;
    destination: string;
    onClick?: () => void;
  };
  items: T[];
  onChange?: (selectedItems: T[]) => void;
  tabId?: string;
  messageId?: string;
  classNames?: string[];
}

export abstract class BaseDropdown<T = any> {
  render: ExtendedHTMLElement;
  protected readonly props: BaseDropdownProps<T>;
  protected readonly tabId: string;
  protected readonly messageId: string;
  protected dropdownContent: ExtendedHTMLElement | null = null;
  protected dropdownPortal: ExtendedHTMLElement | null = null;
  protected readonly uid: string;
  protected isOpen = false;
  protected selectedItems: T[] = [];
  protected dropdownIcon: ExtendedHTMLElement;
  protected readonly sheetOpenListenerId: string | null = null;

  // Abstract methods that subclasses must implement
  protected abstract createItemElement (item: T): ExtendedHTMLElement;
  protected abstract handleItemSelection (item: T): void;
  protected abstract getItemSelectionState (item: T): boolean;
  protected abstract getDisplayLabel (): string;

  // Helper method to get CSS variable values with calc() support
  protected getCSSVariableValue (variableName: string, fallback: number): number {
    const value = getComputedStyle(document.documentElement).getPropertyValue(variableName).trim();

    if (value.length > 0) {
      let numericValue: number;

      if (value.includes('calc(')) {
        // For calc expressions, create a temporary element to get the computed value
        const tempDiv = document.createElement('div');
        tempDiv.style.position = 'absolute';
        tempDiv.style.visibility = 'hidden';
        tempDiv.style.width = value;
        document.body.appendChild(tempDiv);
        const computedWidth = getComputedStyle(tempDiv).width;
        document.body.removeChild(tempDiv);
        numericValue = parseFloat(computedWidth.replace(/px$/, ''));
      } else {
        // Remove 'px' suffix if present and parse the numeric value
        const cleanValue = value.replace(/px$/, '');
        numericValue = parseFloat(cleanValue);
      }

      return isNaN(numericValue) ? fallback : numericValue;
    }

    return fallback;
  }

  constructor (props: BaseDropdownProps<T>) {
    this.props = props;
    this.uid = generateUID();

    // Initialize messageId + tabId
    this.tabId = props.tabId ?? '';
    this.messageId = props.messageId ?? '';

    // Initialize selected items
    this.selectedItems = this.getInitialSelection();

    // Initialize the dropdown icon
    this.dropdownIcon = new Icon({ icon: MynahIcons.DOWN_OPEN }).render;

    // Create the main dropdown button with the selected item's label if available
    const initialLabel = this.getDisplayLabel();
    const dropdownButton = new Button({
      label: initialLabel,
      icon: this.dropdownIcon,
      onClick: this.toggleDropdown,
      primary: false,
      status: 'dimmed-clear',
      classNames: [ 'mynah-dropdown-list-button' ],
      testId: testIds.dropdownList.button
    }).render;

    // Create the main container (without dropdown content)
    this.render = DomBuilder.getInstance().build({
      type: 'div',
      testId: testIds.dropdownList.wrapper,
      classNames: [
        'mynah-dropdown-list-wrapper',
        ...(props.classNames ?? [])
      ],
      attributes: {
        id: this.uid
      },
      children: [
        dropdownButton
      ]
    });

    // Add click outside listener to close dropdown (use capture phase to catch events before stopPropagation)
    document.addEventListener('click', this.handleClickOutside, true);
  }

  protected getInitialSelection (): T[] {
    // Default implementation - subclasses can override if needed
    return [];
  }

  protected readonly updateUI = (): void => {
    // Update dropdown items (if dropdown is open)
    if (this.dropdownContent != null) {
      const itemElements = this.dropdownContent.querySelectorAll('[data-item-id]');

      Array.from(itemElements).forEach((element) => {
        const itemElement = element as ExtendedHTMLElement;
        const itemId = itemElement.getAttribute('data-item-id');
        if (itemId == null) return;

        const item = this.props.items.find(item => this.getItemId(item) === itemId);
        if (item == null) return;

        // Replace the entire element with updated version
        const updatedElement = this.createItemElement(item);
        itemElement.replaceWith(updatedElement);
      });
    }

    // Update button label
    const buttonLabel = this.render.querySelector('.mynah-dropdown-list-button .mynah-button-label');
    if (buttonLabel != null) {
      buttonLabel.innerHTML = this.getDisplayLabel();
    }
  };

  protected abstract getItemId (item: T): string;

  protected readonly onLinkClick = (buttonId: string): void => {
    MynahUIGlobalEvents.getInstance().dispatch(MynahEventNames.DROPDOWN_LINK_CLICK, {
      tabId: this.props.tabId,
      actionId: buttonId,
      destination: this.props.descriptionLink?.destination
    });
  };

  protected readonly toggleDropdown = (e: Event): void => {
    e.stopPropagation();
    this.isOpen = !this.isOpen;
    this.isOpen ? this.openDropdown() : this.closeDropdown();
  };

  protected readonly openDropdown = (): void => {
    // Create the dropdown content
    this.dropdownContent = DomBuilder.getInstance().build({
      type: 'div',
      classNames: [ 'mynah-dropdown-list-content', 'open' ],
      children: [
        {
          type: 'div',
          classNames: [ 'mynah-dropdown-list-options' ],
          children: this.props.items.map(item => this.createItemElement(item))
        },
        {
          type: 'div',
          classNames: [ 'mynah-dropdown-list-footer' ],
          children: [
            ...((this.props.description != null && this.props.description.trim() !== '') || this.props.descriptionLink != null
              ? [ {
                  type: 'p',
                  testId: testIds.dropdownList.description,
                  classNames: [ 'mynah-dropdown-list-description' ],
                  children: [
                    ...(this.props.description != null && this.props.description.trim() !== ''
                      ? [ this.props.description ]
                      : []),
                    ...(this.props.descriptionLink != null
                      ? (() => {
                          const descriptionLink = this.props.descriptionLink;
                          return [ {
                            type: 'button',
                            classNames: [ 'mynah-dropdown-list-description-link' ],
                            events: {
                              click: (e: Event) => {
                                e.stopPropagation();
                                this.onLinkClick(descriptionLink.id);
                              }
                            },
                            children: [ descriptionLink.text ]
                          } ];
                        })()
                      : [])
                  ]
                } ]
              : [])
          ]
        },
      ]
    });

    // Create portal container
    this.dropdownPortal = DomBuilder.getInstance().createPortal(
      `${MynahPortalNames.SHEET}-dropdown-${this.uid}`,
      {
        type: 'div',
        testId: testIds.dropdownList.portal,
        classNames: [ 'mynah-dropdown-list-portal' ],
        events: {
          click: (event: MouseEvent) => {
            // Prevent closing when clicking inside the dropdown
            event.stopPropagation();
          }
        },
        children: [ this.dropdownContent ]
      },
      'beforeend'
    );

    // Position the dropdown and add scroll listeners
    this.updateDropdownPosition();
    window.addEventListener('scroll', this.updateDropdownPosition, true);
    window.addEventListener('resize', this.updateDropdownPosition);

    // Update the icon to UP_OPEN when the dropdown is open
    this.dropdownIcon.replaceWith(new Icon({ icon: MynahIcons.UP_OPEN }).render);
    this.dropdownIcon = this.render.querySelector('.mynah-dropdown-list-button .mynah-ui-icon') as ExtendedHTMLElement;
  };

  protected readonly isElementVisible = (element: Element): boolean => {
    const rect = element.getBoundingClientRect();

    // Check viewport bounds first (quick check)
    const viewportHeight = window.innerHeight ?? document.documentElement.clientHeight;
    const viewportWidth = window.innerWidth ?? document.documentElement.clientWidth;
    if (rect.bottom < 0 || rect.top > viewportHeight || rect.right < 0 || rect.left > viewportWidth) {
      return false;
    }

    // Check parent containers with overflow
    for (let parent = element.parentElement; parent != null; parent = parent.parentElement) {
      const parentStyle = window.getComputedStyle(parent);
      const hasOverflow = [ 'overflow', 'overflowX', 'overflowY' ].some(
        prop => parentStyle[prop as any] !== 'visible'
      );

      if (hasOverflow) {
        const parentRect = parent.getBoundingClientRect();
        if (rect.bottom < parentRect.top || rect.top > parentRect.bottom ||
            rect.right < parentRect.left || rect.left > parentRect.right) {
          return false;
        }
      }
    }

    return true;
  };

  protected readonly updateDropdownPosition = (): void => {
    if (this.dropdownPortal == null) return;

    // Close dropdown if button is not visible
    if (!this.isElementVisible(this.render)) {
      this.isOpen = false;
      this.closeDropdown();
      return;
    }

    // Calculate position using CSS variables
    const buttonRect = this.render.getBoundingClientRect();
    const dropdownWidth = this.getCSSVariableValue('--mynah-dropdown-width', 250);
    const dropdownMargin = this.getCSSVariableValue('--mynah-dropdown-margin', 4);

    // Position dropdown below button with margin
    const calculatedTop = buttonRect.bottom + dropdownMargin;

    // Align with chat item card if present, otherwise align with button
    const chatItemCard = this.render.closest('.mynah-chat-item-card');
    const calculatedLeft = (chatItemCard != null)
      ? chatItemCard.getBoundingClientRect().right - dropdownWidth
      : buttonRect.right - dropdownWidth;

    // Update position
    this.dropdownPortal.style.top = `${calculatedTop}px`;
    this.dropdownPortal.style.left = `${calculatedLeft}px`;
  };

  protected readonly closeDropdown = (): void => {
    // Remove scroll and resize listeners
    window.removeEventListener('scroll', this.updateDropdownPosition, true);
    window.removeEventListener('resize', this.updateDropdownPosition);

    // Remove the portal
    if (this.dropdownPortal != null) {
      this.dropdownPortal.remove();
      this.dropdownPortal = null;
    }
    this.dropdownContent = null;

    // Update the icon to DOWN_OPEN when the dropdown is closed
    this.dropdownIcon.replaceWith(new Icon({ icon: MynahIcons.DOWN_OPEN }).render);
    this.dropdownIcon = this.render.querySelector('.mynah-dropdown-list-button .mynah-ui-icon') as ExtendedHTMLElement;
  };

  protected readonly handleClickOutside = (e: MouseEvent): void => {
    if (!this.isOpen) return;

    const target = e.target as Node;

    // Don't close if clicking inside the dropdown portal
    if (this.dropdownPortal?.contains(target) ?? false) {
      return;
    }

    // Don't close if clicking on this dropdown's button
    if (this.render.contains(target)) {
      return;
    }

    // Close the dropdown for any other click
    this.isOpen = false;
    this.closeDropdown();
    this.dispatchChangeEvent();
  };

  public readonly getSelectedItems = (): T[] => {
    return [ ...this.selectedItems ];
  };

  public readonly setSelectedItems = (itemIds: string[]): void => {
    this.selectedItems = this.props.items.filter(item =>
      itemIds.includes(this.getItemId(item))
    );
    this.updateUI();
  };

  protected readonly dispatchChangeEvent = (): void => {
    MynahUIGlobalEvents.getInstance().dispatch(MynahEventNames.DROPDOWN_OPTION_CHANGE, {
      value: this.selectedItems,
      messageId: this.messageId,
      tabId: this.tabId
    });

    // Also trigger onChange callback if provided
    if (this.props.onChange != null) {
      this.props.onChange(this.selectedItems);
    }
  };

  public readonly destroy = (): void => {
    document.removeEventListener('click', this.handleClickOutside, true);

    // Remove sheet open listener if it exists
    if (this.sheetOpenListenerId != null) {
      MynahUIGlobalEvents.getInstance().removeListener(MynahEventNames.OPEN_SHEET, this.sheetOpenListenerId);
    }

    // Remove scroll and resize listeners if dropdown is open
    if (this.isOpen) {
      window.removeEventListener('scroll', this.updateDropdownPosition, true);
      window.removeEventListener('resize', this.updateDropdownPosition);
    }

    // Clean up portal if it exists
    if (this.dropdownPortal != null) {
      this.dropdownPortal.remove();
      this.dropdownPortal = null;
    }
    this.dropdownContent = null;
  };
}
