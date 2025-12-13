import { ExtendedHTMLElement } from '../../../helper/dom';
import { cancelEvent, MynahUIGlobalEvents } from '../../../helper/events';
import { MynahUITabsStore } from '../../../helper/tabs-store';
import testIds from '../../../helper/test-ids';
import { MynahEventNames, ProgressField } from '../../../static';
import { ProgressIndicator } from '../../progress';

export interface PromptInputProgressProps {
  tabId: string;
}

export class PromptInputProgress {
  render: ExtendedHTMLElement;
  private readonly progressIndicator: ProgressIndicator;
  private progressData: ProgressField;
  private readonly props: PromptInputProgressProps;
  constructor (props: PromptInputProgressProps) {
    this.props = props;
    this.progressData = MynahUITabsStore.getInstance().getTabDataStore(this.props.tabId).getValue('promptInputProgress') ?? {};
    this.progressIndicator = new ProgressIndicator({
      testId: testIds.prompt.progress,
      classNames: [ 'mynah-prompt-input-progress-field' ],
      ...this.progressData,
      onActionClick: (action, e) => {
        if (e != null) {
          cancelEvent(e);
        }
        MynahUIGlobalEvents.getInstance().dispatch(MynahEventNames.PROMPT_PROGRESS_ACTION_CLICK, {
          tabId: this.props.tabId,
          actionId: action.id,
          actionText: action.text
        });
      }
    });
    this.render = this.progressIndicator.render;

    MynahUITabsStore.getInstance().getTabDataStore(this.props.tabId).subscribe('promptInputProgress', (progressData) => {
      this.progressData = progressData;
      if (this.progressData === null) {
        this.progressIndicator.update(null);
      } else {
        this.progressIndicator.update(this.progressData);
      }
    });
  }
}
