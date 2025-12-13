import { DomBuilder, ExtendedHTMLElement } from '../../helper/dom';
import { MynahUIGlobalEvents, cancelEvent } from '../../helper/events';
import { FileNodeAction, MynahEventNames, TreeNodeDetails } from '../../static';
import { Button } from '../button';
import { Card } from '../card/card';
import { CardBody } from '../card/card-body';
import { Icon, MynahIcons, MynahIconsType } from '../icon';
import { Overlay, OverlayHorizontalDirection, OverlayVerticalDirection } from '../overlay';
import testIds from '../../helper/test-ids';
import { parseMarkdown } from '../../helper/marked';

export interface ChatItemTreeFileProps {
  tabId: string;
  messageId: string;
  filePath: string;
  originalFilePath: string;
  fileName: string;
  icon?: MynahIcons | MynahIconsType | null;
  deleted?: boolean;
  details?: TreeNodeDetails;
  actions?: FileNodeAction[];
}

const PREVIEW_DELAY = 250;
export class ChatItemTreeFile {
  render: ExtendedHTMLElement;
  private readonly props: ChatItemTreeFileProps;
  private fileTooltip: Overlay | null;
  private fileTooltipTimeout: ReturnType<typeof setTimeout>;
  constructor (props: ChatItemTreeFileProps) {
    this.props = props;
    this.render = DomBuilder.getInstance().build({
      type: 'div',
      testId: testIds.chatItem.fileTree.file,
      classNames: [
        'mynah-chat-item-tree-view-file-item',
        'mynah-button', 'mynah-button-secondary',
        this.props.details?.clickable === false ? 'mynah-chat-item-tree-view-not-clickable' : '',
        this.props.details?.status != null ? `mynah-chat-item-tree-view-file-item-status-${this.props.details?.status}` : '',
      ],
      events: {
        click: () => {
          this.hideTooltip();
          if (this.props.details?.clickable !== false) {
            MynahUIGlobalEvents.getInstance().dispatch(MynahEventNames.FILE_CLICK, {
              tabId: this.props.tabId,
              messageId: this.props.messageId,
              filePath: this.props.originalFilePath,
              deleted: this.props.deleted,
              fileDetails: this.props.details
            });
          }
        },
        mouseover: (e) => {
          cancelEvent(e);
          const textContentSpan: HTMLSpanElement | null = this.render.querySelector('.mynah-chat-item-tree-view-file-item-title-text');
          let tooltipText;
          if (textContentSpan != null && textContentSpan.offsetWidth < textContentSpan.scrollWidth) {
            tooltipText = parseMarkdown(this.props.fileName, { includeLineBreaks: true });
          }
          if (this.props.details?.description != null) {
            if (tooltipText != null) {
              tooltipText += '\n\n';
            } else {
              tooltipText = '';
            }
            tooltipText += parseMarkdown(this.props.details?.description ?? '', { includeLineBreaks: true });
          }
          if (tooltipText != null) {
            this.showTooltip(tooltipText, undefined, OverlayHorizontalDirection.START_TO_RIGHT, textContentSpan);
          }
        },
        mouseleave: this.hideTooltip
      },
      children: [
        ...(this.props.icon != null && this.props.details?.icon === undefined
          ? [ {
              type: 'span',
              classNames: [ 'mynah-chat-single-file-icon' ],
              children: [ new Icon({ icon: this.props.icon }).render ]
            } ]
          : []),
        {
          type: 'div',
          classNames: [
            'mynah-chat-item-tree-view-file-item-title',
            this.props.deleted === true ? 'mynah-chat-item-tree-view-file-item-deleted' : '',
          ],
          children: [
            ...(this.props.details?.icon !== null ? [ new Icon({ icon: this.props.details?.icon ?? MynahIcons.FILE, status: this.props.details?.iconForegroundStatus }).render ] : []),
            {
              type: 'span',
              classNames: [ 'mynah-chat-item-tree-view-file-item-title-text' ],
              children: [ this.props.details?.visibleName ?? this.props.fileName ]
            } ]
        },
        {
          type: 'div',
          classNames: [ 'mynah-chat-item-tree-view-file-item-details' ],
          children: this.props.details != null
            ? [
                ...(this.props.details.changes != null
                  ? [ {
                      type: 'span',
                      classNames: [ 'mynah-chat-item-tree-view-file-item-details-changes' ],
                      children: [
                        ...(this.props.details.changes.added != null ? [ { type: 'span', classNames: [ 'changes-added' ], children: [ `+${this.props.details.changes.added}` ] } ] : []),
                        ...(this.props.details.changes.deleted != null ? [ { type: 'span', classNames: [ 'changes-deleted' ], children: [ `-${this.props.details.changes.deleted}` ] } ] : []),
                        ...(this.props.details.changes.total != null ? [ { type: 'span', classNames: [ 'changes-total' ], children: [ `${this.props.details.changes.total}` ] } ] : []),
                      ]
                    } ]
                  : []),
                ...(this.props.details?.labelIcon != null ? [ new Icon({ icon: this.props.details?.labelIcon, status: this.props.details?.labelIconForegroundStatus }).render ] : []),
                ...(this.props.details.label != null
                  ? [ {
                      type: 'span',
                      classNames: [ 'mynah-chat-item-tree-view-file-item-details-text' ],
                      children: [
                        this.props.details.label
                      ]
                    } ]
                  : []),
              ]
            : []
        },
        ...(this.props.actions !== undefined
          ? [ {
              type: 'div',
              classNames: [ 'mynah-chat-item-tree-view-file-item-actions' ],
              children: this.props.actions.map((action: FileNodeAction) => new Button({
                testId: testIds.chatItem.fileTree.fileAction,
                icon: new Icon({ icon: action.icon }).render,
                ...(action.label !== undefined ? { label: action.label } : {}),
                attributes: {
                  title: action.description ?? ''
                },
                classNames: [ 'mynah-icon-button', action.status ?? '' ],
                primary: false,
                onClick: (e) => {
                  cancelEvent(e);
                  this.hideTooltip();
                  MynahUIGlobalEvents.getInstance().dispatch(MynahEventNames.FILE_ACTION_CLICK, {
                    tabId: this.props.tabId,
                    messageId: this.props.messageId,
                    filePath: this.props.originalFilePath,
                    actionName: action.name,
                  });
                },
              }).render)
            } ]
          : []),
      ]
    });
  }

  private readonly showTooltip = (content: string, vDir?: OverlayVerticalDirection, hDir?: OverlayHorizontalDirection, elm?: null | HTMLElement | ExtendedHTMLElement): void => {
    if (content.trim() !== '') {
      clearTimeout(this.fileTooltipTimeout);
      this.fileTooltipTimeout = setTimeout(() => {
        clearTimeout(this.fileTooltipTimeout);
        this.fileTooltip = new Overlay({
          testId: testIds.chatItem.fileTree.fileTooltipWrapper,
          background: true,
          closeOnOutsideClick: false,
          referenceElement: elm ?? this.render,
          dimOutside: false,
          removeOtherOverlays: true,
          verticalDirection: vDir ?? OverlayVerticalDirection.TO_TOP,
          horizontalDirection: hDir ?? OverlayHorizontalDirection.CENTER,
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
      }, PREVIEW_DELAY);
    }
  };

  public readonly hideTooltip = (): void => {
    if (this.fileTooltipTimeout != null) {
      clearTimeout(this.fileTooltipTimeout);
    }
    this.fileTooltip?.close();
    this.fileTooltip = null;
  };
}
