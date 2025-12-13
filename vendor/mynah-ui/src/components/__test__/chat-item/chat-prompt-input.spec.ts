import { ChatPromptInput } from '../../chat-item/chat-prompt-input';

// Mock the tabs store
jest.mock('../../../helper/tabs-store', () => ({
  MynahUITabsStore: {
    getInstance: jest.fn(() => ({
      getTabDataStore: jest.fn(() => ({
        subscribe: jest.fn(),
        getValue: jest.fn((key: string) => {
          // Return appropriate string values for string-expected keys
          if (key === 'promptInputText' || key === 'promptInputLabel' || key === 'promptInputInfo') {
            return '';
          }
          // Return false for boolean keys
          return false;
        }),
        updateStore: jest.fn()
      })),
      addListenerToDataStore: jest.fn()
    }))
  }
}));

describe('ChatPromptInput', () => {
  it('should render chat prompt input', () => {
    const input = new ChatPromptInput({
      tabId: 'test-tab'
    });

    expect(input.render).toBeDefined();
  });
});
