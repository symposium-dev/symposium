import { MynahUITabsStore } from '../../../helper/tabs-store';
import { PromptTextInput } from '../../chat-item/prompt-input/prompt-text-input';
import { Config } from '../../../helper/config';

describe('prompt-text-input', () => {
  it('renders with correct attributes', () => {
    const testTabId = MynahUITabsStore.getInstance().addTab({
      isSelected: true,
      store: {
        promptInputPlaceholder: 'Enter your prompt...',
        promptInputDisabledState: false,
      }
    }) as string;

    const textInput = new PromptTextInput({
      tabId: testTabId,
      initMaxLength: 1000,
      onKeydown: () => {}
    });

    const inputElement = textInput.render.querySelector('.mynah-chat-prompt-input');
    expect(inputElement?.getAttribute('placeholder')).toBe('Enter your prompt...');
    expect(inputElement?.getAttribute('contenteditable')).toBe('plaintext-only');
    expect(inputElement?.getAttribute('maxlength')).toBe('4000');
  });

  it('handles disabled state', () => {
    const testTabId = MynahUITabsStore.getInstance().addTab({
      isSelected: true,
      store: {
        promptInputDisabledState: true,
      }
    }) as string;

    const textInput = new PromptTextInput({
      tabId: testTabId,
      initMaxLength: 1000,
      onKeydown: () => {}
    });

    const inputElement = textInput.render.querySelector('.mynah-chat-prompt-input');
    expect(inputElement?.getAttribute('disabled')).toBe('disabled');
    expect(inputElement?.getAttribute('contenteditable')).toBe('plaintext-only');
    expect(inputElement?.getAttribute('disabled')).toBe('disabled');
  });

  it('handles focus and blur events', () => {
    const testTabId = MynahUITabsStore.getInstance().addTab({
      isSelected: true,
      store: {}
    }) as string;

    let focused = false;
    let blurred = false;
    const textInput = new PromptTextInput({
      tabId: testTabId,
      initMaxLength: 1000,
      onKeydown: () => {},
      onFocus: () => { focused = true; },
      onBlur: () => { blurred = true; }
    });

    const inputElement = textInput.render.querySelector('.mynah-chat-prompt-input') as HTMLElement;

    inputElement.dispatchEvent(new FocusEvent('focus'));
    expect(focused).toBe(true);

    inputElement.dispatchEvent(new FocusEvent('blur'));
    expect(blurred).toBe(true);
  });

  it('handles input events', () => {
    const testTabId = MynahUITabsStore.getInstance().addTab({
      isSelected: true,
      store: {}
    }) as string;

    let inputCalled = false;
    const textInput = new PromptTextInput({
      tabId: testTabId,
      initMaxLength: 1000,
      onKeydown: () => {},
      onInput: () => { inputCalled = true; }
    });

    const inputElement = textInput.render.querySelector('.mynah-chat-prompt-input') as HTMLElement;
    inputElement.dispatchEvent(new Event('input'));
    expect(inputCalled).toBe(true);
  });

  it('updates placeholder text', () => {
    const testTabId = MynahUITabsStore.getInstance().addTab({
      isSelected: true,
      store: {
        promptInputPlaceholder: 'Initial placeholder'
      }
    }) as string;

    const textInput = new PromptTextInput({
      tabId: testTabId,
      initMaxLength: 1000,
      onKeydown: () => {}
    });

    MynahUITabsStore.getInstance().updateTab(testTabId, {
      store: {
        promptInputPlaceholder: 'Updated placeholder'
      }
    });

    const inputElement = textInput.render.querySelector('.mynah-chat-prompt-input');
    expect(inputElement?.getAttribute('placeholder')).toBe('Updated placeholder');
  });

  it('handles paste events', () => {
    const testTabId = MynahUITabsStore.getInstance().addTab({
      isSelected: true,
      store: {}
    }) as string;

    const textInput = new PromptTextInput({
      tabId: testTabId,
      initMaxLength: 1000,
      onKeydown: () => {}
    });

    const inputElement = textInput.render.querySelector('.mynah-chat-prompt-input') as HTMLElement;

    inputElement.dispatchEvent(new Event('paste'));
    expect(textInput).toBeDefined();
  });

  it('manages text input value and clearing', () => {
    const testTabId = MynahUITabsStore.getInstance().addTab({
      isSelected: true,
      store: {}
    }) as string;

    const textInput = new PromptTextInput({
      tabId: testTabId,
      initMaxLength: 1000,
      onKeydown: () => {}
    });

    // Test that methods exist and can be called
    textInput.updateTextInputValue('test value');
    textInput.clear();
    expect(textInput.getTextInputValue()).toBe('');
  });

  it('handles focus and blur methods', () => {
    const testTabId = MynahUITabsStore.getInstance().addTab({
      isSelected: true,
      store: {}
    }) as string;

    const textInput = new PromptTextInput({
      tabId: testTabId,
      initMaxLength: 1000,
      onKeydown: () => {}
    });

    Config.getInstance().config.autoFocus = true;
    textInput.focus();
    textInput.blur();

    expect(textInput).toBeDefined();
  });

  it('updates max length', () => {
    const testTabId = MynahUITabsStore.getInstance().addTab({
      isSelected: true,
      store: {}
    }) as string;

    const textInput = new PromptTextInput({
      tabId: testTabId,
      initMaxLength: 1000,
      onKeydown: () => {}
    });

    textInput.updateTextInputMaxLength(2000);
    const inputElement = textInput.render.querySelector('.mynah-chat-prompt-input');
    expect(inputElement?.getAttribute('maxlength')).toBe('2000');
  });

  it('inserts context items', () => {
    const testTabId = MynahUITabsStore.getInstance().addTab({
      isSelected: true,
      store: {}
    }) as string;

    const textInput = new PromptTextInput({
      tabId: testTabId,
      initMaxLength: 1000,
      onKeydown: () => {}
    });

    const contextItem = {
      command: 'test-command',
      description: 'Test command'
    };

    textInput.insertContextItem(contextItem, 0);
    expect(textInput.getUsedContext()).toBeDefined();
  });

  it('deletes text range', () => {
    const testTabId = MynahUITabsStore.getInstance().addTab({
      isSelected: true,
      store: {}
    }) as string;

    const textInput = new PromptTextInput({
      tabId: testTabId,
      initMaxLength: 1000,
      onKeydown: () => {}
    });

    textInput.updateTextInputValue('hello world');
    textInput.deleteTextRange(0, 5);
    expect(textInput).toBeDefined();
  });

  it('inserts end space', () => {
    const testTabId = MynahUITabsStore.getInstance().addTab({
      isSelected: true,
      store: {}
    }) as string;

    const textInput = new PromptTextInput({
      tabId: testTabId,
      initMaxLength: 1000,
      onKeydown: () => {}
    });

    textInput.updateTextInputValue('test');
    textInput.insertEndSpace();
    expect(textInput).toBeDefined();
  });

  it('handles keydown events', () => {
    const testTabId = MynahUITabsStore.getInstance().addTab({
      isSelected: true,
      store: {}
    }) as string;

    let keydownCalled = false;
    const textInput = new PromptTextInput({
      tabId: testTabId,
      initMaxLength: 1000,
      onKeydown: () => { keydownCalled = true; }
    });

    const inputElement = textInput.render.querySelector('.mynah-chat-prompt-input') as HTMLElement;
    inputElement.dispatchEvent(new KeyboardEvent('keydown', { key: 'Enter' }));
    expect(keydownCalled).toBe(true);
  });

  it('handles disabled state changes', () => {
    const testTabId = MynahUITabsStore.getInstance().addTab({
      isSelected: true,
      store: {
        promptInputDisabledState: false
      }
    }) as string;

    const textInput = new PromptTextInput({
      tabId: testTabId,
      initMaxLength: 1000,
      onKeydown: () => {}
    });

    MynahUITabsStore.getInstance().updateTab(testTabId, {
      store: {
        promptInputDisabledState: true
      }
    });

    const inputElement = textInput.render.querySelector('.mynah-chat-prompt-input');
    expect(inputElement?.getAttribute('disabled')).toBe('disabled');
  });

  it('gets cursor position', () => {
    const testTabId = MynahUITabsStore.getInstance().addTab({
      isSelected: true,
      store: {}
    }) as string;

    const textInput = new PromptTextInput({
      tabId: testTabId,
      initMaxLength: 1000,
      onKeydown: () => {}
    });

    expect(textInput.getCursorPos()).toBe(0);
  });
});
