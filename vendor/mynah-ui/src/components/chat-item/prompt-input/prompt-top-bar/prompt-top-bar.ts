import { DomBuilder, ExtendedHTMLElement } from '../../../../helper/dom';
import { MynahUIGlobalEvents } from '../../../../helper/events';
import { convertQuickActionCommandGroupsToDetailedListGroups } from '../../../../helper/quick-pick-data-handler';
import testIds from '../../../../helper/test-ids';
import { ChatItemButton, DetailedList, DetailedListItemGroup, MynahEventNames, QuickActionCommand } from '../../../../static';
import { Button } from '../../../button';
import { DetailedListWrapper } from '../../../detailed-list/detailed-list';
import { Icon, MynahIcons } from '../../../icon';
import { Overlay, OverlayHorizontalDirection, OverlayVerticalDirection } from '../../../overlay';
import { TopBarButton } from './top-bar-button';

const PREVIEW_DELAY = 500;

export interface PromptTopBarProps {
  classNames?: string[];
  tabId: string;
  contextItems?: QuickActionCommand[];
  title?: string;

  onTopBarTitleClick?: () => void;
  onContextItemAdd?: (contextItems: QuickActionCommand) => void;
  onContextItemRemove?: (contextItems: QuickActionCommand) => void;

  topBarButton?: ChatItemButton;
  onTopBarButtonClick?: (action: ChatItemButton) => void;
}

export class PromptTopBar {
  render: ExtendedHTMLElement;
  visibleCount: number;
  overflowOverlay?: Overlay;
  topBarButton: TopBarButton;
  overflowListContainer: DetailedListWrapper;
  private contextTooltip: Overlay | null;
  private contextTooltipTimeout: ReturnType<typeof setTimeout>;
  private readonly props: PromptTopBarProps;
  private titleButton: Button;
  private overflowButton: ExtendedHTMLElement;

  constructor (props: PromptTopBarProps) {
    this.props = props;
    this.visibleCount = (this.props.contextItems != null) ? this.props.contextItems?.length : 0;

    this.topBarButton = new TopBarButton({
      topBarButton: this.props.topBarButton,
      onTopBarButtonClick: this.props.onTopBarButtonClick
    });

    this.render = DomBuilder.getInstance().build({
      type: 'div',
      testId: testIds.prompt.topBar,
      classNames: [ 'mynah-prompt-input-top-bar', ...(this.props.classNames ?? []), this.isHidden() ? 'hidden' : '' ],
      children: [
        this.generateTitle(),
        ...this.generateContextPills(),
        this.generateOverflowPill(),
        this.topBarButton.render
      ],

    });

    // Add resize observer to handle responsive behavior
    this.setupResizeObserver();

    MynahUIGlobalEvents.getInstance().addListener(MynahEventNames.CONTEXT_PINNED, (data) => {
      if (data.tabId === props.tabId) {
        this.addContextPill(data.contextItem);
      }
    });

    // Use setTimeout to ensure the DOM is fully rendered before measuring
    // TODO: Switch to an IntersectionObserver
    setTimeout(() => {
      this.recalculateVisibleItems();
    }, 100);
  }

  update (newProps?: Partial<PromptTopBarProps>): void {
    if (newProps?.contextItems != null) {
      this.props.contextItems = newProps.contextItems;
    }
    if (newProps?.title != null) {
      this.props.title = newProps.title;
    }

    if (newProps?.topBarButton != null) {
      this.topBarButton.update({ topBarButton: newProps.topBarButton });
    }

    this.render.update({
      children: [
        this.generateTitle(),
        ...this.generateContextPills(),
        this.generateOverflowPill(),
        this.topBarButton.render
      ]

    });

    if (this.isHidden()) {
      this.render.addClass('hidden');
    } else {
      this.render.removeClass('hidden');
    }

    if (newProps?.contextItems != null || newProps?.title != null) {
      this.recalculateVisibleItems();
    }
  }

  updateTopBarButtonOverlay (topBarButtonOverlay: DetailedList): void {
    this.topBarButton.onTopBarButtonOverlayChanged(topBarButtonOverlay);
  }

  isHidden (): boolean {
    return this.props.title == null || this.props.title.length === 0;
  }

  generateTitle (): ExtendedHTMLElement | string {
    const { title } = this.props;
    if (title == null) {
      return '';
    }
    if (this.titleButton == null) {
      this.titleButton = new Button({
        onClick: () => {
          this.props.onTopBarTitleClick?.();
        },
        primary: false,
        status: 'clear',
        border: false,
        label: title,
        hidden: title == null
      });
    } else {
      this.titleButton.updateLabel(title);
    }
    return this.titleButton.render;
  }

  getVisibleContextItems (): QuickActionCommand[] {
    return this.props.contextItems?.slice(0, this.visibleCount) ?? [];
  }

  getOverflowContextItems (): QuickActionCommand[] {
    return this.props.contextItems?.slice(this.visibleCount) ?? [];
  }

  generateContextPills (): Array<ExtendedHTMLElement | string> {
    if ((this.props.contextItems != null) && this.props.contextItems?.length > 0) {
      return this.getVisibleContextItems().map((contextItem) => {
        return DomBuilder.getInstance().build({
          type: 'span',
          testId: testIds.prompt.topBarContextPill,
          children: [
            new Icon({ icon: MynahIcons.CANCEL, classNames: [ 'hover-icon' ] }).render,
            new Icon({ icon: contextItem.icon ?? MynahIcons.AT }).render,
            { type: 'span', classNames: [ 'label' ], children: [ `${contextItem.command.replace(/^@?(.*)$/, '$1')}` ] }
          ],
          classNames: [ 'pinned-context-pill' ],
          attributes: {
            contenteditable: 'false'
          },
          events: {
            mouseenter: (e) => {
              this.showContextTooltip(e, contextItem);
            },
            mouseleave: (e) => {
              this.hideContextTooltip();
            },
            click: (e) => {
              this.hideContextTooltip();
              this.removeContextPill(contextItem.id ?? contextItem.command);
            }
          }
        });
      });
    }
    return [];
  }

  private readonly showContextTooltip = (e: MouseEvent, contextItem: QuickActionCommand): void => {
    clearTimeout(this.contextTooltipTimeout);
    this.contextTooltipTimeout = setTimeout(() => {
      const elm: HTMLElement = e.target as HTMLElement;

      this.contextTooltip = new Overlay({
        background: true,
        closeOnOutsideClick: false,
        referenceElement: elm,
        dimOutside: false,
        removeOtherOverlays: true,
        verticalDirection: OverlayVerticalDirection.TO_TOP,
        horizontalDirection: OverlayHorizontalDirection.START_TO_RIGHT,
        children: [
          DomBuilder.getInstance().build({
            type: 'div',
            testId: testIds.prompt.topBarContextTooltip,
            classNames: [ 'mynah-chat-prompt-context-tooltip' ],
            children: [
              ...(contextItem.icon !== undefined
                ? [
                    new Icon({
                      icon: contextItem.icon
                    }).render
                  ]
                : []),
              {
                type: 'div',
                classNames: [ 'mynah-chat-prompt-context-tooltip-container' ],
                children: [
                  {
                    type: 'div',
                    classNames: [ 'mynah-chat-prompt-context-tooltip-name' ],
                    children: [ contextItem.command ]
                  },
                  ...(contextItem.description !== undefined
                    ? [ {
                        type: 'div',
                        classNames: [ 'mynah-chat-prompt-context-tooltip-description' ],
                        children: [ contextItem.description ]
                      } ]
                    : [])
                ]
              }
            ]
          })
        ],
      });
    }, PREVIEW_DELAY);
  };

  private readonly hideContextTooltip = (): void => {
    if (this.contextTooltipTimeout !== null) {
      clearTimeout(this.contextTooltipTimeout);
    }
    if (this.contextTooltip != null) {
      this.contextTooltip.close();
      this.contextTooltip = null;
    }
  };

  removeContextPill (id: string): void {
    const itemToRemove = this.props.contextItems?.find((item) => (item.id ?? item.command) === id);
    if (itemToRemove != null) {
      this.props.contextItems = this.props.contextItems?.filter((item) => (item.id ?? item.command) !== id);
      this.props.onContextItemRemove?.(itemToRemove);
      this.update();
      this.recalculateVisibleItems();
      this.overflowOverlay?.updateContent([ this.generateOverflowOverlayChildren() ]);
    }
  }

  addContextPill (contextItem: QuickActionCommand): void {
    if (this.props.contextItems?.find(({ id }) => id != null && id === contextItem.id) == null) {
      this.props.contextItems?.push(contextItem);
      this.props.onContextItemAdd?.(contextItem);
      this.update();
      this.recalculateVisibleItems();
    }
  }

  getOverflowCount (): number {
    return (this.props.contextItems?.length ?? 0) - this.visibleCount;
  }

  generateOverflowPill (): ExtendedHTMLElement | string {
    if (this.getOverflowCount() <= 0) {
      return '';
    }
    if (this.overflowButton == null) {
      this.overflowButton = DomBuilder.getInstance().build({
        type: 'span',
        testId: testIds.prompt.topBarOverflowPill,
        children: [
        `+${this.getOverflowCount()}`
        ],
        classNames: [ 'pinned-context-pill', 'overflow-button' ],
        attributes: {
          contenteditable: 'false'
        },
        events: {
          click: (e: Event) => {
            this.showOverflowOverlay(e);
          }
        }
      });
    } else {
      this.overflowButton.update({
        children: [
        `+${this.getOverflowCount()}`
        ]
      });
    }

    return this.overflowButton;
  }

  showOverflowOverlay (e: Event): void {
    if (this.overflowOverlay == null) {
      this.overflowOverlay = new Overlay({
        testId: testIds.prompt.topBarOverflowOverlay,
        background: true,
        closeOnOutsideClick: true,
        referenceElement: this.overflowButton,
        removeIfReferenceElementRemoved: false,
        dimOutside: false,
        onClose: () => { this.overflowOverlay = undefined; },
        removeOtherOverlays: true,
        verticalDirection: OverlayVerticalDirection.TO_TOP,
        horizontalDirection: OverlayHorizontalDirection.END_TO_LEFT,
        children: [ this.generateOverflowOverlayChildren() ]
      });
    } else {
      this.overflowOverlay.updateContent([ this.generateOverflowOverlayChildren() ]);
    }
  }

  generateOverflowOverlayChildren (): ExtendedHTMLElement {
    const overflowItems = this.getOverflowItemsAsDetailedListGroup();
    if (this.overflowListContainer == null) {
      this.overflowListContainer = new DetailedListWrapper({
        detailedList: { list: overflowItems, selectable: 'clickable' },
        onItemActionClick: (_, item) => {
          const itemId = item?.id ?? item?.title;
          if (itemId != null) { this.removeContextPill(itemId); }
        },
        onItemClick: (item) => {
          const itemId = item.id ?? item.title;
          if (itemId != null) { this.removeContextPill(itemId); }
        },
      });
    } else {
      if (overflowItems.length === 0 || overflowItems[0].children?.length === 0) {
        this.overflowOverlay?.close();
      } else {
        this.overflowListContainer.update({ list: overflowItems });
      }
    }
    return DomBuilder.getInstance().build({
      type: 'div',
      classNames: [ 'mynah-chat-prompt-quick-picks-overlay-wrapper' ],
      children: [ this.overflowListContainer.render ]
    }); ;
  }

  getOverflowItemsAsDetailedListGroup (): DetailedListItemGroup[] {
    return convertQuickActionCommandGroupsToDetailedListGroups([ { commands: this.getOverflowContextItems() } ]).map((group) => ({
      ...group,
      children: group.children?.map((child) => ({ ...child, actions: [ { icon: MynahIcons.CANCEL, id: 'remove' } ] }))
    }));
  }

  private setupResizeObserver (): void {
    MynahUIGlobalEvents.getInstance().addListener(MynahEventNames.ROOT_RESIZE, () => {
      this.recalculateVisibleItems();
    });
  }

  // Sets visibleContextItems based on container width. Pills that don't fit will be moved into context overflow.
  // As width increases, move items back from context overflow into row of displayed pills.
  private recalculateVisibleItems (): void {
    const { contextItems } = this.props;
    if ((contextItems == null) || contextItems.length === 0) return;

    const containerWidth = this.render.offsetWidth;
    const titleWidth = (this.titleButton != null) ? this.titleButton.render.offsetWidth + 8 : 0; // 8px for margin/padding
    const topBarButtonWidth = (this.topBarButton != null) ? this.topBarButton.render.offsetWidth + 8 : 0;

    // Available width for context pills
    const availableWidth = containerWidth - titleWidth - topBarButtonWidth - 16; // 16px for container padding

    // Check if we need to handle width increase scenario
    const shouldCheckForExpansion = this.getOverflowCount() > 0;
    if (shouldCheckForExpansion) {
      // Try to add one more item from overflow if we have at least 100px of extra space
      const extraSpaceNeeded = 100; // Maximum width for a pill to be brought back

      // Calculate current used width
      let currentUsedWidth = 0;
      const currentPills = Array.from(this.render.querySelectorAll('.pinned-context-pill'));
      currentPills.forEach((pill) => {
        currentUsedWidth += (pill as HTMLElement).offsetWidth + 8; // 8px for gap
      });

      // Check if we have enough space to bring back an item from overflow
      const remainingWidth = availableWidth - currentUsedWidth;
      if (remainingWidth >= extraSpaceNeeded && this.getOverflowCount() > 0) {
        // We have enough space to bring back at least one item
        this.visibleCount++;

        // Rebuild the component with the new visible items
        this.update();
        return; // Exit early as we've updated the component
      }
    }

    // Handle width decrease scenario
    // Get all context pills
    const contextPills = Array.from(this.render.querySelectorAll('.pinned-context-pill:not(.overflow-button)'));

    // Calculate how many pills can fit
    let usedWidth = 0;
    let visibleCount = 0;

    for (let i = 0; i < contextPills.length; i++) {
      const pill = contextPills[i] as HTMLElement;
      usedWidth += pill.offsetWidth + 8; // 8px for gap

      if (usedWidth > availableWidth) {
        break;
      }

      visibleCount++;
    }
    // If we need to adjust the visible items
    if (this.visibleCount !== visibleCount) {
      this.visibleCount = visibleCount;

      // Rebuild the component with the new visible items

      this.update();
    }
  }
}
