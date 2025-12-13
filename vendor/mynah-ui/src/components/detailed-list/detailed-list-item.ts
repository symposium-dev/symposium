import { DomBuilder, ExtendedHTMLElement } from '../../helper/dom';
import { cancelEvent } from '../../helper/events';
import testIds from '../../helper/test-ids';
import { ChatItemButton, DetailedListItem } from '../../static';
import { Button } from '../button';
import { Icon, MynahIcons } from '../icon';
import { Overlay, OverlayHorizontalDirection, OverlayVerticalDirection } from '../overlay';
import { parseMarkdown } from '../../helper/marked';
import { Card } from '../card/card';
import { CardBody } from '../card/card-body';

const TOOLTIP_DELAY = 350;

export interface DetailedListItemWrapperProps {
  listItem: DetailedListItem;
  descriptionTextDirection?: 'ltr' | 'rtl';
  onSelect?: (detailedListItem: DetailedListItem) => void;
  onClick?: (detailedListItem: DetailedListItem) => void;
  onActionClick?: (action: ChatItemButton, detailedListItem?: DetailedListItem) => void;
  onShowActionMenuOverlay?: () => void;
  selectable?: boolean;
  clickable?: boolean;
  textDirection?: 'row' | 'column';
}

export class DetailedListItemWrapper {
  render: ExtendedHTMLElement;
  private tooltipOverlay: Overlay | null;
  private tooltipTimeout: ReturnType<typeof setTimeout>;
  private readonly props: DetailedListItemWrapperProps;
  private actionMenuOverlay: Overlay | undefined;

  constructor (props: DetailedListItemWrapperProps) {
    this.props = props;
    this.render = DomBuilder.getInstance().build({
      type: 'div',
      testId: testIds.prompt.quickPickItem,
      classNames: [ 'mynah-detailed-list-item' ],
      attributes: {
        disabled: this.props.listItem.disabled ?? 'false',
        selectable: this.props.selectable ?? 'true',
        clickable: this.props.clickable ?? 'false',
      },
      events: {
        // Prevent mousedown from stealing focus from the input
        mousedown: (e) => { cancelEvent(e); },
        click: (e) => {
          cancelEvent(e);
          if (this.props.listItem.disabled !== true && this.props.selectable !== false) {
            this.props.onSelect?.(this.props.listItem);
          }
          if (this.props.listItem.disabled !== true && this.props.clickable !== false) {
            this.props.onClick?.(this.props.listItem);
          }
        }
      },
      children: [
        ...(this.props.listItem.icon != null
          ? [
              {
                type: 'div',
                classNames: [ 'mynah-detailed-list-icon' ],
                children: [
                  new Icon({
                    icon: this.props.listItem.icon,
                    status: this.props.listItem.iconForegroundStatus
                  }).render
                ]
              }
            ]
          : []),
        {
          type: 'div',
          classNames: [ 'mynah-detailed-list-item-text', 'mynah-detailed-list-item-text-direction-' + (this.props.textDirection ?? 'row') ],
          children: [
            ...(this.props.listItem.title != null || this.props.listItem.name != null
              ? [ {
                  type: 'div',
                  classNames: [ 'mynah-detailed-list-item-name' ],
                  innerHTML: this.props.listItem.title ?? this.props.listItem.name
                } ]
              : []),
            ...(this.props.listItem.description != null
              ? [ {
                  type: 'div',
                  classNames: [ 'mynah-detailed-list-item-description', this.props.descriptionTextDirection ?? 'ltr' ],
                  innerHTML: `<bdi>${parseMarkdown(this.props.listItem.description.replace(/ /g, '&nbsp;').replace(/\n\s*\n/g, ' '), { includeLineBreaks: false, inline: true })}</bdi>`
                } ]
              : [])
          ]
        },
        ...(this.props.listItem.disabledText != null
          ? [
              {
                type: 'div',
                classNames: [ 'mynah-detailed-list-item-disabled-text' ],
                children: [ `(${this.props.listItem.disabledText})` ]
              }
            ]
          : (this.props.listItem.children != null) && this.props.listItem.children.length > 0
              ? [
                  {
                    type: 'div',
                    classNames: [ 'mynah-detailed-list-item-arrow-icon' ],
                    children: [
                      new Icon({ icon: 'right-open' }).render
                    ]
                  }
                ]
              : []),
        ...(this.props.listItem.status != null
          ? [
              DomBuilder.getInstance().build({
                type: 'div',
                classNames: [
                  'mynah-detailed-list-item-status',
                `status-${this.props.listItem.status.status ?? 'default'}`,
                ],
                children: [
                  ...(this.props.listItem.status.text != null ? [ { type: 'span', children: [ this.props.listItem.status.text ] } ] : []),
                  ...(this.props.listItem.status.icon != null ? [ new Icon({ icon: this.props.listItem.status.icon }).render ] : []),
                ],
                ...(this.props.listItem.status.description != null
                  ? {
                      events: {
                        mouseover: (e) => {
                          cancelEvent(e);
                          this.showTooltip(e.currentTarget, parseMarkdown(this.props.listItem.status?.description ?? '', { includeLineBreaks: true }));
                        },
                        mouseleave: this.hideTooltip
                      }
                    }
                  : {})
              })
            ]
          : []),
        ...(this.props.listItem.actions != null
          ? this.props.listItem.groupActions !== false && this.props.listItem.actions.length > 1
            ? [ {
                type: 'div',
                classNames: [ 'mynah-detailed-list-item-actions' ],
                children: [ new Button({
                  testId: testIds.detailedList.actionMenu,
                  icon: new Icon({ icon: MynahIcons.ELLIPSIS }).render,
                  primary: false,
                  onClick: (e) => {
                    cancelEvent(e);
                    this.showActionMenuOverlay(this.props.listItem);
                  },
                }).render ]
              } ]
            : [ {
                type: 'div',
                classNames: [ 'mynah-detailed-list-item-actions' ],
                children: this.props.listItem.actions.map((action) => this.getActionButton(action, (this.props.listItem.groupActions === false), this.props.listItem))
              } ]
          : []),
      ]
    });
  }

  private readonly showTooltip = (elm: HTMLElement, content: string): void => {
    if (content.trim() !== undefined) {
      clearTimeout(this.tooltipTimeout);
      this.tooltipTimeout = setTimeout(() => {
        this.tooltipOverlay = new Overlay({
          background: true,
          closeOnOutsideClick: false,
          referenceElement: elm,
          dimOutside: false,
          removeOtherOverlays: false,
          verticalDirection: OverlayVerticalDirection.TO_TOP,
          horizontalDirection: OverlayHorizontalDirection.CENTER,
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

  public readonly setFocus = (isFocused: boolean, scrollIntoView: boolean): void => {
    if (isFocused) {
      this.render.addClass('target-command');
      if (scrollIntoView) {
        this.render.scrollIntoView(true);
      }
    } else {
      this.render.removeClass('target-command');
    }
  };

  public readonly getItem = (): DetailedListItem => {
    return this.props.listItem;
  };

  private readonly showActionMenuOverlay = (listItem?: DetailedListItem): void => {
    this.props.onShowActionMenuOverlay?.();
    this.actionMenuOverlay = new Overlay({
      background: true,
      closeOnOutsideClick: true,
      referenceElement: this.render,
      dimOutside: false,
      removeOtherOverlays: true,
      verticalDirection: OverlayVerticalDirection.CENTER,
      horizontalDirection: OverlayHorizontalDirection.END_TO_LEFT,
      children: [
        {
          type: 'div',
          classNames: [ 'mynah-detailed-list-item-actions-overlay' ],
          children: this.props.listItem.actions?.map((action) => this.getActionButton(action, true, listItem))
        }
      ],
    });
  };

  private getActionButton (action: ChatItemButton, showText?: boolean, listItem?: DetailedListItem): ExtendedHTMLElement {
    return DomBuilder.getInstance().build({
      type: 'div',
      classNames: [ 'mynah-detailed-list-item-actions-item' ],
      children: [
        new Button({
          testId: testIds.detailedList.action,
          icon: action.icon ? new Icon({ icon: action.icon }).render : undefined,
          ...(showText === true ? { label: action.text } : {}),
          tooltip: action.description,
          disabled: action.disabled,
          primary: false,
          border: false,
          confirmation: action.confirmation,
          status: action.status,
          onClick: (e) => {
            cancelEvent(e);
            this.props.onActionClick?.(action, listItem);
            this.hideActionMenuOverlay();
          },
        }).render
      ]
    });
  }

  private readonly hideActionMenuOverlay = (): void => {
    if (this.actionMenuOverlay !== undefined) {
      this.actionMenuOverlay.close();
      this.actionMenuOverlay = undefined;
    }
  };
}
