import { ExtendedHTMLElement } from '../../../helper/dom';
import { Overlay, OverlayHorizontalDirection, OverlayVerticalDirection } from '../../overlay';
import { Icon, MynahIcons } from '../../icon';
import { Button } from '../../button';
import { MynahUIGlobalEvents, cancelEvent } from '../../../helper/events';
import { MynahEventNames, PromptAttachmentType } from '../../../static';
import { Card } from '../../card/card';
import { CardBody } from '../../card/card-body';
import { SyntaxHighlighter } from '../../syntax-highlighter';
import testIds from '../../../helper/test-ids';

export interface PromptTextAttachmentProps {
  tabId: string;
  content: string;
  type: PromptAttachmentType;
}

export class PromptTextAttachment {
  render: ExtendedHTMLElement;
  private readonly props: PromptTextAttachmentProps;
  private previewOverlay: Overlay | undefined;
  constructor (props: PromptTextAttachmentProps) {
    this.props = props;
    this.render = new Card({
      testId: testIds.prompt.attachment,
      padding: 'none',
      border: false,
      events: {
        mouseenter: () => {
          this.showPreviewOverLay();
        },
        mouseleave: () => {
          this.closePreviewOverLay();
        },
      },
      classNames: [ 'mynah-prompt-attachment-container' ],
      children: [
        new CardBody({
          ...(this.props.type === 'markdown'
            ? { body: this.props.content }
            : {
                children: [ new SyntaxHighlighter({
                  block: true,
                  codeStringWithMarkup: this.props.content,
                }).render ]
              }),
        }).render,
        new Button({
          testId: testIds.prompt.attachmentRemove,
          classNames: [ 'code-snippet-close-button' ],
          onClick: e => {
            cancelEvent(e);
            MynahUIGlobalEvents.getInstance().dispatch(MynahEventNames.REMOVE_ATTACHMENT, this.props.tabId);
            this.closePreviewOverLay();
          },
          icon: new Icon({ icon: MynahIcons.CANCEL }).render,
          primary: false,
        }).render,
      ],
    }).render;
  }

  private readonly showPreviewOverLay = (): void => {
    this.previewOverlay = new Overlay({
      background: true,
      closeOnOutsideClick: false,
      referenceElement: this.render,
      dimOutside: false,
      removeOtherOverlays: true,
      verticalDirection: OverlayVerticalDirection.TO_TOP,
      horizontalDirection: OverlayHorizontalDirection.START_TO_RIGHT,
      children: [
        new Card({
          border: false,
          classNames: [ 'mynah-prompt-input-snippet-attachment-overlay' ],
          children: [
            new CardBody({
              ...(this.props.type === 'markdown'
                ? { body: this.props.content }
                : {
                    children: [ new SyntaxHighlighter({
                      block: true,
                      codeStringWithMarkup: this.props.content,
                    }).render ]
                  }),
            }).render,
          ]
        }).render
      ],
    });
  };

  private readonly closePreviewOverLay = (): void => {
    if (this.previewOverlay !== undefined) {
      this.previewOverlay.close();
      this.previewOverlay = undefined;
    }
  };

  public readonly clear = (): void => {
    this.closePreviewOverLay();
  };
}
