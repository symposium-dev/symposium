import { ChatItemType, MynahUI, MynahUIDataModel } from '../main';

const testTabId = 'tab-1';
let testMynahUI: MynahUI;
afterEach(() => {
  // Clear the dom and store between tests
  document.body.innerHTML = '';
  testMynahUI.updateStore(testTabId, {
    loadingChat: false,
    chatItems: [],
  });
});

describe('mynah-ui', () => {
  it('render chat items', () => {
    const testTabId = 'tab-1';
    testMynahUI = new MynahUI({
      tabs: {
        [testTabId]: {
          isSelected: true,
          store: {
            loadingChat: false,
          }
        }
      }
    });

    testMynahUI.addChatItem(testTabId, { type: ChatItemType.PROMPT, body: 'What is a react hook' });
    testMynahUI.addChatItem(testTabId, { type: ChatItemType.ANSWER_STREAM });
    testMynahUI.updateLastChatAnswer(testTabId, { body: 'Just a function.' });
    testMynahUI.addChatItem(testTabId, {
      type: ChatItemType.ANSWER,
      followUp: {
        text: 'Suggested follow up',
        options: [
          {
            pillText: 'Follow up one',
            prompt: 'Follow up one',
          },
        ],
      },
    });
    const cardElements = document.body.querySelectorAll('.mynah-chat-item-card');
    expect(cardElements).toHaveLength(3);
    expect(cardElements[0].textContent).toBe('What is a react hook');
    expect(cardElements[1].textContent).toContain('Just a function.');
    expect(cardElements[2].textContent).toContain('Suggested follow up');
    expect(cardElements[2].textContent).toContain('Follow up one');
  });

  it('loading state', () => {
    const testMynahUI = new MynahUI({
      tabs: {
        [testTabId]: {
          isSelected: true,
          store: {
            loadingChat: false,
          }
        }
      }
    });

    testMynahUI.addChatItem(testTabId, { type: ChatItemType.PROMPT, body: 'What is python' });
    // Still generating an answer
    testMynahUI.addChatItem(testTabId, { type: ChatItemType.ANSWER_STREAM });
    testMynahUI.updateStore(testTabId, {
      loadingChat: true,
    });

    const cardElements = document.body.querySelectorAll('.mynah-chat-item-card');
    expect(cardElements).toHaveLength(2);

    expect(cardElements[0].textContent).toBe('What is python');
    expect(cardElements[1].textContent).toBe('Amazon Q is generating your answer...');
  });

  it('does not break on data store extension', () => {
    const testMynahUI = new MynahUI({
      tabs: {
        [testTabId]: {
          isSelected: true,
          store: {
            loadingChat: false,
          }
        }
      }
    });

    type ExtendedDataModel = MynahUIDataModel & { someOtherProperty: boolean };
    const props: ExtendedDataModel = { someOtherProperty: true };

    try {
      testMynahUI.updateStore(testTabId, props);
    } catch (e) {
      console.log(e);
      expect(true).toBe(false);
    }
  });
});
