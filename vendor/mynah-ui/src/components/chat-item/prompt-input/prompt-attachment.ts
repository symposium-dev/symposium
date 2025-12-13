import { DomBuilder, ExtendedHTMLElement } from '../../../helper/dom';
import { MynahUITabsStore } from '../../../helper/tabs-store';
import { PromptAttachmentType } from '../../../static';
import { PromptTextAttachment } from './prompt-text-attachment';

export interface PromptAttachmentProps {
  tabId: string;
}

export class PromptAttachment {
  render: ExtendedHTMLElement;
  lastAttachmentContent: string = '';
  private readonly props: PromptAttachmentProps;
  private attachmentItem: PromptTextAttachment | undefined;
  constructor (props: PromptAttachmentProps) {
    this.props = props;

    this.render = DomBuilder.getInstance().build({
      type: 'div',
      classNames: [ 'outer-container' ],
      persistent: true,
    });
  }

  public readonly updateAttachment = (attachmentContent: string | undefined, type?: PromptAttachmentType): void => {
    if (this.attachmentItem !== undefined) {
      this.attachmentItem.clear();
    }
    this.render.clear();
    this.lastAttachmentContent = attachmentContent != null
      ? (type === 'code'
          ? `
~~~~~~~~~~
${attachmentContent}
~~~~~~~~~~`
          : `
${attachmentContent}
`)
      : '';
    if (attachmentContent !== undefined && attachmentContent !== '') {
      this.attachmentItem = new PromptTextAttachment({
        tabId: this.props.tabId,
        content: attachmentContent,
        type: type ?? 'markdown'
      });
      this.render.insertChild('afterbegin', this.attachmentItem.render);
      const isCodeOverflowVertically =
        (this.render.getBoundingClientRect()?.height ?? 0) < (this.render.getElementsByTagName('code')?.[0]?.getBoundingClientRect()?.height ?? 0);
      if (isCodeOverflowVertically) {
        this.render.children[0].classList.add('vertical-overflow');
      }
    }
    MynahUITabsStore.getInstance().getTabDataStore(this.props.tabId)?.updateStore({
      selectedCodeSnippet: attachmentContent,
    });
  };

  public readonly clear = (): void => {
    this.lastAttachmentContent = '';
    if (this.attachmentItem !== undefined) {
      this.attachmentItem.clear();
    }
    this.attachmentItem = undefined;
    this.render.clear();
    MynahUITabsStore.getInstance().getTabDataStore(this.props.tabId)?.updateStore({
      selectedCodeSnippet: undefined,
    });
  };
}
