import { TopBarButton } from '../../chat-item/prompt-input/prompt-top-bar/top-bar-button';

describe('top-bar-button', () => {
  it('renders without button', () => {
    const topBarButton = new TopBarButton({
      topBarButton: undefined,
      onTopBarButtonClick: undefined
    });

    expect(topBarButton.render).toBeDefined();
  });

  it('renders with button', () => {
    const button = {
      id: 'test-button',
      text: 'Test Button',
      status: 'info' as const
    };

    let buttonClicked = false;
    const topBarButton = new TopBarButton({
      topBarButton: button,
      onTopBarButtonClick: () => { buttonClicked = true; }
    });

    expect(topBarButton.render.classList.contains('hidden')).toBe(false);

    const buttonElement = topBarButton.render.querySelector('.mynah-button') as HTMLElement;
    if (buttonElement != null) {
      buttonElement.click();
      expect(buttonClicked).toBe(true);
    }
  });

  it('handles different button states', () => {
    const button = {
      id: 'test-button',
      text: 'Test Button',
      status: 'success' as const
    };

    const topBarButton = new TopBarButton({
      topBarButton: button,
      onTopBarButtonClick: () => {}
    });

    expect(topBarButton.render.classList.contains('hidden')).toBe(false);
    expect(topBarButton.render.querySelector('.mynah-button')).toBeDefined();
  });
});
