import { Config } from '../../../helper/config';
import { ExtendedHTMLElement } from '../../../helper/dom';
import { MynahUITabsStore } from '../../../helper/tabs-store';
import testIds from '../../../helper/test-ids';
import { Button } from '../../button';
import { Icon, MynahIcons } from '../../icon';
import { OverlayHorizontalDirection } from '../../overlay';

export interface PromptInputStopButtonPrompts {
  tabId: string;
  onClick: () => void;
}

export class PromptInputStopButton {
  render: ExtendedHTMLElement;
  private readonly props: PromptInputStopButtonPrompts;
  constructor (props: PromptInputStopButtonPrompts) {
    this.props = props;
    const tabStore = MynahUITabsStore.getInstance().getTabDataStore(this.props.tabId);

    MynahUITabsStore.getInstance().addListenerToDataStore(this.props.tabId, 'cancelButtonWhenLoading', (isVisible) => {
      this.checkVisibilityState(isVisible, tabStore.getValue('loadingChat'));
    });
    MynahUITabsStore.getInstance().addListenerToDataStore(this.props.tabId, 'loadingChat', (isLoading) => {
      this.checkVisibilityState(tabStore.getValue('cancelButtonWhenLoading'), isLoading);
    });

    this.render = new Button({
      testId: testIds.prompt.send,
      classNames: [
        'mynah-chat-prompt-stop-button', 'hidden'
      ],
      attributes: {
        tabindex: '0'
      },
      label: Config.getInstance().config.texts.stopGenerating,
      icon: new Icon({ icon: MynahIcons.STOP }).render,
      primary: false,
      border: false,
      tooltip: Config.getInstance().config.texts.stopGeneratingTooltip ?? Config.getInstance().config.texts.stopGenerating,
      tooltipHorizontalDirection: OverlayHorizontalDirection.END_TO_LEFT,
      status: 'clear',
      onClick: () => {
        this.props.onClick();
      },
    }).render;
    this.checkVisibilityState(tabStore.getValue('cancelButtonWhenLoading'), tabStore.getValue('loadingChat'));
  }

  private readonly checkVisibilityState = (isVisible: boolean, loadingState: boolean): void => {
    if (isVisible && loadingState) {
      this.render.removeClass('hidden');
    } else {
      this.render.addClass('hidden');
    }
  };
}
