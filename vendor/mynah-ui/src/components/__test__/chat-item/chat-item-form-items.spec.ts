import { ChatItemFormItemsWrapper } from '../../chat-item/chat-item-form-items';
import { ChatItem, ChatItemType } from '../../../static';

describe('ChatItemFormItemsWrapper', () => {
  it('should render form items wrapper', () => {
    const wrapper = new ChatItemFormItemsWrapper({
      tabId: 'test-tab',
      chatItem: {}
    });

    expect(wrapper.render).toBeDefined();
  });

  it('should disable radio input when disabled is true', () => {
    const chatItem: ChatItem = {
      type: ChatItemType.PROMPT,
      formItems: [
        {
          id: 'test-radio',
          type: 'radiogroup',
          title: 'Test Radio',
          options: [ { value: 'option1', label: 'Option 1' } ],
          disabled: true
        }
      ]
    };

    const wrapper = new ChatItemFormItemsWrapper({
      tabId: 'test-tab',
      chatItem
    });

    const radioInput = wrapper.render.querySelector('.mynah-form-input[disabled]');
    expect(radioInput?.hasAttribute('disabled')).toBe(true);
  });

  it('should disable text input when disabled is true', () => {
    const chatItem: ChatItem = {
      type: ChatItemType.PROMPT,
      formItems: [
        {
          id: 'test-text',
          type: 'textinput',
          title: 'Test Text',
          disabled: true
        }
      ]
    };

    const wrapper = new ChatItemFormItemsWrapper({
      tabId: 'test-tab',
      chatItem
    });

    const textInput = wrapper.render.querySelector('input[type="text"][disabled]');
    expect(textInput?.hasAttribute('disabled')).toBe(true);
  });

  it('should disable numeric input when disabled is true', () => {
    const chatItem: ChatItem = {
      type: ChatItemType.PROMPT,
      formItems: [
        {
          id: 'test-numeric',
          type: 'numericinput',
          title: 'Test Numeric',
          disabled: true
        }
      ]
    };

    const wrapper = new ChatItemFormItemsWrapper({
      tabId: 'test-tab',
      chatItem
    });

    const numericInput = wrapper.render.querySelector('input[type="number"][disabled]');
    expect(numericInput?.hasAttribute('disabled')).toBe(true);
  });

  it('should disable email input when disabled is true', () => {
    const chatItem: ChatItem = {
      type: ChatItemType.PROMPT,
      formItems: [
        {
          id: 'test-email',
          type: 'email',
          title: 'Test Email',
          disabled: true
        }
      ]
    };

    const wrapper = new ChatItemFormItemsWrapper({
      tabId: 'test-tab',
      chatItem
    });

    const emailInput = wrapper.render.querySelector('input[type="email"][disabled]');
    expect(emailInput?.hasAttribute('disabled')).toBe(true);
  });

  it('should not disable form items when disabled is false or undefined', () => {
    const chatItem: ChatItem = {
      type: ChatItemType.PROMPT,
      formItems: [
        {
          id: 'test-enabled',
          type: 'textinput',
          title: 'Test Enabled',
          disabled: false
        }
      ]
    };

    const wrapper = new ChatItemFormItemsWrapper({
      tabId: 'test-tab',
      chatItem
    });

    const textInput = wrapper.render.querySelector('input[type="text"]');
    expect(textInput?.hasAttribute('disabled')).toBe(false);
  });
});
