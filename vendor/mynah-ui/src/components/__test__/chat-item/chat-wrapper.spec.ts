import { ChatWrapper } from '../../chat-item/chat-wrapper';

// Mock the tabs store
jest.mock('../../../helper/tabs-store', () => ({
  MynahUITabsStore: {
    getInstance: jest.fn(() => ({
      getTabDataStore: jest.fn(() => ({
        subscribe: jest.fn(),
        getValue: jest.fn((key: string) => {
          // Return appropriate string values for string-expected keys
          if (key === 'promptInputInfo' || key === 'promptInputLabel' || key === 'promptInputText') {
            return '';
          }
          // Return empty object for other keys
          return {};
        }),
        updateStore: jest.fn()
      })),
      addListenerToDataStore: jest.fn()
    }))
  }
}));

describe('ChatWrapper', () => {
  it('should render chat wrapper', () => {
    const wrapper = new ChatWrapper({
      tabId: 'test-tab'
    });

    expect(wrapper.render).toBeDefined();
  });
});
