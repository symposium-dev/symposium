import { MynahUITabsStore } from '../../../helper/tabs-store';
import { MynahUIGlobalEvents } from '../../../helper/events';
import { PromptInputProgress } from '../../chat-item/prompt-input/prompt-progress';

describe('prompt-input-progress', () => {
  it('renders with progress data', () => {
    const testTabId = MynahUITabsStore.getInstance().addTab({
      isSelected: true,
      store: {
        promptInputProgress: {
          value: 75,
          text: 'Processing request...',
          actions: [ {
            id: 'cancel',
            text: 'Cancel'
          } ]
        }
      }
    }) as string;

    const progressInput = new PromptInputProgress({
      tabId: testTabId
    });

    expect(progressInput.render.querySelector('[data-testid="prompt-input-progress-wrapper"]')).toBeDefined();
  });

  it('handles progress updates', () => {
    const testTabId = MynahUITabsStore.getInstance().addTab({
      isSelected: true,
      store: {
        promptInputProgress: null
      }
    }) as string;

    const progressInput = new PromptInputProgress({
      tabId: testTabId
    });

    MynahUITabsStore.getInstance().updateTab(testTabId, {
      store: {
        promptInputProgress: {
          value: 50,
          text: 'Updated progress'
        }
      }
    });

    expect(progressInput.render).toBeDefined();
  });

  it('handles action clicks', () => {
    const testTabId = MynahUITabsStore.getInstance().addTab({
      isSelected: true,
      store: {
        promptInputProgress: {
          value: 25,
          text: 'Loading...',
          actions: [ {
            id: 'stop',
            text: 'Stop'
          } ]
        }
      }
    }) as string;

    const originalDispatch = MynahUIGlobalEvents.getInstance().dispatch;
    MynahUIGlobalEvents.getInstance().dispatch = () => {};

    const progressInput = new PromptInputProgress({
      tabId: testTabId
    });
    expect(progressInput).toBeDefined();

    MynahUIGlobalEvents.getInstance().dispatch = originalDispatch;
  });
});
