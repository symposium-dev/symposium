import { PromptTopBar } from '../../../../../components/chat-item/prompt-input/prompt-top-bar/prompt-top-bar';

jest.mock('../../../../../helper/events', () => ({
  MynahUIGlobalEvents: { getInstance: () => ({ addListener: jest.fn() }) }
}));

jest.mock('../../../../../components/overlay', () => ({
  Overlay: jest.fn(() => ({ close: jest.fn() })),
  OverlayHorizontalDirection: { END_TO_LEFT: 'end-to-left' },
  OverlayVerticalDirection: { TO_TOP: 'to-top' }
}));

jest.mock('../../../../../components/detailed-list/detailed-list', () => ({
  DetailedListWrapper: jest.fn(() => ({ render: document.createElement('div') }))
}));

jest.mock('../../../../../components/chat-item/prompt-input/prompt-top-bar/top-bar-button', () => ({
  TopBarButton: jest.fn(() => ({ render: document.createElement('div') }))
}));

describe('PromptTopBar Overflow', () => {
  it('should show overflow overlay', () => {
    const promptTopBar = new PromptTopBar({ tabId: 'test', title: 'Test' });
    promptTopBar.showOverflowOverlay(new Event('click'));
    expect(jest.requireMock('../../../../../components/overlay').Overlay).toHaveBeenCalled();
  });
});
