import { MynahUIGlobalEvents } from '../../../helper/events';
import { MynahEventNames } from '../../../static';
import { FeedbackForm } from '../../feedback-form/feedback-form';

describe('feedback form', () => {
  afterEach(() => {
    jest.clearAllMocks();
  });

  it('submit', () => {
    const testFeedbackForm = new FeedbackForm({
      initPayload: {
        selectedOption: 'buggy-code',
        comment: 'test comment',
        messageId: 'test-message-id',
        tabId: 'test-tab-id',
      }
    });

    const spyDispatch = jest.spyOn(MynahUIGlobalEvents.getInstance(), 'dispatch');

    // Actually render the portal
    MynahUIGlobalEvents.getInstance().dispatch(MynahEventNames.SHOW_FEEDBACK_FORM, {
      messageId: 'test-message-id',
      tabId: 'test-tab-id',
    });

    const submitButtonElement = testFeedbackForm.defaultFeedbackFormItems[testFeedbackForm.defaultFeedbackFormItems.length - 1].querySelectorAll('button')[1];
    expect(submitButtonElement.textContent).toBe('Submit');
    submitButtonElement.click();
    expect(spyDispatch).toHaveBeenCalledTimes(4);
    expect(spyDispatch).toHaveBeenNthCalledWith(1, MynahEventNames.SHOW_FEEDBACK_FORM, {
      messageId: 'test-message-id',
      tabId: 'test-tab-id',
    });
  });

  it('cancel', () => {
    const testFeedbackForm = new FeedbackForm({
      initPayload: {
        selectedOption: 'buggy-code',
        comment: 'test comment',
        messageId: 'test-message-id',
        tabId: 'test-tab-id',
      }
    });

    const spyDispatch = jest.spyOn(MynahUIGlobalEvents.getInstance(), 'dispatch');

    // Actually render the portal
    MynahUIGlobalEvents.getInstance().dispatch(MynahEventNames.SHOW_FEEDBACK_FORM, {
      messageId: 'test-message-id',
      tabId: 'test-tab-id',
    });

    const cancelButtonElement = testFeedbackForm.defaultFeedbackFormItems[testFeedbackForm.defaultFeedbackFormItems.length - 1].querySelectorAll('button')[0];
    expect(cancelButtonElement.textContent).toBe('Cancel');
    cancelButtonElement.click();
    expect(spyDispatch).toHaveBeenCalledTimes(4);
    expect(spyDispatch).toHaveBeenNthCalledWith(1, MynahEventNames.SHOW_FEEDBACK_FORM, {
      messageId: 'test-message-id',
      tabId: 'test-tab-id',
    });
  });
});
