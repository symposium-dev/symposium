import { MynahUITabsStore } from '../../../helper/tabs-store';
import { PromptInputSendButton } from '../../chat-item/prompt-input/prompt-input-send-button';

describe('prompt-input-send-button', () => {
  it('renders with correct attributes', () => {
    const testTabId = MynahUITabsStore.getInstance().addTab({
      isSelected: true,
      store: {
        promptInputDisabledState: false,
      }
    }) as string;

    let clicked = false;
    const sendButton = new PromptInputSendButton({
      tabId: testTabId,
      onClick: () => { clicked = true; }
    });

    expect(sendButton.render.querySelector('[data-testid="prompt-input-send-button"]')).toBeDefined();
    expect(sendButton.render.getAttribute('disabled')).toBe(null);

    sendButton.render.click();
    expect(clicked).toBe(true);
  });

  it('handles disabled state changes', () => {
    const testTabId = MynahUITabsStore.getInstance().addTab({
      isSelected: true,
      store: {
        promptInputDisabledState: true,
      }
    }) as string;

    const sendButton = new PromptInputSendButton({
      tabId: testTabId,
      onClick: () => {}
    });

    expect(sendButton.render.getAttribute('disabled')).toBe('disabled');

    MynahUITabsStore.getInstance().updateTab(testTabId, {
      store: { promptInputDisabledState: false }
    });

    expect(sendButton.render.getAttribute('disabled')).toBe(null);
  });
});
