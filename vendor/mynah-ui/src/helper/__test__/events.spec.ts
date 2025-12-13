import { MynahEventNames } from '../../static';
import { MynahUIGlobalEvents } from '../events';

describe('events', () => {
  it('addListener', () => {
    const mockData = 'mockData';
    const mockCopyToClipBoardEventHandler = jest.fn();
    MynahUIGlobalEvents.getInstance().addListener(MynahEventNames.COPY_CODE_TO_CLIPBOARD, mockCopyToClipBoardEventHandler);
    const mockCardVoteEventHandler = jest.fn();
    MynahUIGlobalEvents.getInstance().addListener(MynahEventNames.CARD_VOTE, mockCardVoteEventHandler);

    MynahUIGlobalEvents.getInstance().dispatch(MynahEventNames.COPY_CODE_TO_CLIPBOARD, mockData);

    // Only COPY_CODE_TO_CLIPBOARD event's handler should have been called
    expect(mockCardVoteEventHandler.mock.calls).toHaveLength(0);
    expect(mockCopyToClipBoardEventHandler.mock.calls).toHaveLength(1);
    expect(mockCopyToClipBoardEventHandler.mock.calls[0][0]).toBe(mockData);
  });

  it('removeListener', () => {
    const mockData = 'mockData';
    const mockCopyToClipBoardEventHandler = jest.fn();
    const mockEventListenerId = MynahUIGlobalEvents.getInstance().addListener(MynahEventNames.COPY_CODE_TO_CLIPBOARD, mockCopyToClipBoardEventHandler);
    MynahUIGlobalEvents.getInstance().dispatch(MynahEventNames.COPY_CODE_TO_CLIPBOARD, mockData);
    MynahUIGlobalEvents.getInstance().removeListener(MynahEventNames.COPY_CODE_TO_CLIPBOARD, mockEventListenerId);
    MynahUIGlobalEvents.getInstance().dispatch(MynahEventNames.COPY_CODE_TO_CLIPBOARD, mockData);

    // Should only have been called once
    expect(mockCopyToClipBoardEventHandler.mock.calls).toHaveLength(1);
    expect(mockCopyToClipBoardEventHandler.mock.calls[0][0]).toBe(mockData);
  });
});
