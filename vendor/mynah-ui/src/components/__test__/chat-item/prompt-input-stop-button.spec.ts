import { MynahUITabsStore } from '../../../helper/tabs-store';
import { PromptInputStopButton } from '../../chat-item/prompt-input/prompt-input-stop-button';

describe('prompt-input-stop-button', () => {
  it('renders with correct visibility states', () => {
    const testTabId = MynahUITabsStore.getInstance().addTab({
      isSelected: true,
      store: {
        cancelButtonWhenLoading: false,
        loadingChat: false,
      }
    }) as string;

    let clicked = false;
    const stopButton = new PromptInputStopButton({
      tabId: testTabId,
      onClick: () => { clicked = true; }
    });

    expect(stopButton.render.classList.contains('hidden')).toBe(true);

    stopButton.render.click();
    expect(clicked).toBe(true);
  });

  it('shows when loading and cancel button enabled', () => {
    const testTabId = MynahUITabsStore.getInstance().addTab({
      isSelected: true,
      store: {
        cancelButtonWhenLoading: true,
        loadingChat: true,
      }
    }) as string;

    const stopButton = new PromptInputStopButton({
      tabId: testTabId,
      onClick: () => {}
    });

    expect(stopButton.render.classList.contains('hidden')).toBe(false);
  });

  it('hides when not loading', () => {
    const testTabId = MynahUITabsStore.getInstance().addTab({
      isSelected: true,
      store: {
        cancelButtonWhenLoading: true,
        loadingChat: false,
      }
    }) as string;

    const stopButton = new PromptInputStopButton({
      tabId: testTabId,
      onClick: () => {}
    });

    expect(stopButton.render.classList.contains('hidden')).toBe(true);
  });
});
