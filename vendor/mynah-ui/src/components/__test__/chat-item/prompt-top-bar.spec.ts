import { MynahUITabsStore } from '../../../helper/tabs-store';
import { PromptTopBar } from '../../chat-item/prompt-input/prompt-top-bar/prompt-top-bar';

describe('prompt-top-bar', () => {
  it('renders with title', () => {
    const testTabId = MynahUITabsStore.getInstance().addTab({
      isSelected: true,
      store: {}
    }) as string;

    let titleClicked = false;
    const topBar = new PromptTopBar({
      tabId: testTabId,
      title: 'Test Title',
      onTopBarTitleClick: () => { titleClicked = true; }
    });

    expect(topBar.render.querySelector('[data-testid="prompt-input-top-bar"]')).toBeDefined();

    const titleButton = topBar.render.querySelector('button');
    titleButton?.click();
    expect(titleClicked).toBe(true);
  });

  it('renders with context items', () => {
    const testTabId = MynahUITabsStore.getInstance().addTab({
      isSelected: true,
      store: {}
    }) as string;

    const contextItems = [ {
      command: 'test-context',
      description: 'Test Context Item'
    } ];

    const topBar = new PromptTopBar({
      tabId: testTabId,
      contextItems,
      onContextItemAdd: () => {},
      onContextItemRemove: () => {}
    });

    expect(topBar.render.querySelector('.mynah-prompt-input-top-bar')).toBeDefined();
  });

  it('renders with top bar button', () => {
    const testTabId = MynahUITabsStore.getInstance().addTab({
      isSelected: true,
      store: {}
    }) as string;

    const topBarButton = {
      id: 'test-button',
      text: 'Test Button'
    };

    const topBar = new PromptTopBar({
      tabId: testTabId,
      topBarButton,
      onTopBarButtonClick: () => {}
    });

    expect(topBar.topBarButton).toBeDefined();
  });

  it('handles hidden state', () => {
    const testTabId = MynahUITabsStore.getInstance().addTab({
      isSelected: true,
      store: {}
    }) as string;

    const topBar = new PromptTopBar({
      tabId: testTabId
    });

    expect(topBar.render.classList.contains('hidden')).toBe(true);
  });

  it('shows when title or context items provided', () => {
    const testTabId = MynahUITabsStore.getInstance().addTab({
      isSelected: true,
      store: {}
    }) as string;

    const topBar = new PromptTopBar({
      tabId: testTabId,
      title: 'Test Title',
      contextItems: [ {
        command: 'test',
        description: 'Test'
      } ]
    });

    expect(topBar.render.classList.contains('hidden')).toBe(false);
  });
});
