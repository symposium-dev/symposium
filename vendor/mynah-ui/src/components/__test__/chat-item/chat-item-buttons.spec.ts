import { ChatItemButtonsWrapper } from '../../chat-item/chat-item-buttons';
import { ChatItemButton } from '../../../static';

describe('ChatItemButtonsWrapper', () => {
  it('should render empty wrapper when no buttons provided', () => {
    const buttonsWrapper = new ChatItemButtonsWrapper({
      buttons: null
    });

    expect(buttonsWrapper.render).toBeDefined();
    expect(buttonsWrapper.render.classList.contains('mynah-chat-item-buttons-container')).toBe(true);
  });

  it('should render with buttons array', () => {
    const buttons: ChatItemButton[] = [
      {
        id: 'test-button',
        text: 'Test Button',
        description: 'Test button description'
      }
    ];

    const buttonsWrapper = new ChatItemButtonsWrapper({
      buttons,
      tabId: 'test-tab'
    });

    expect(buttonsWrapper.render).toBeDefined();
    expect(buttonsWrapper.render.classList.contains('mynah-chat-item-buttons-container')).toBe(true);
  });

  it('should apply custom class names', () => {
    const customClasses = [ 'custom-class-1', 'custom-class-2' ];

    const buttonsWrapper = new ChatItemButtonsWrapper({
      buttons: [],
      tabId: 'test-tab',
      classNames: customClasses
    });

    expect(buttonsWrapper.render).toBeDefined();
    customClasses.forEach(className => {
      expect(buttonsWrapper.render.classList.contains(className)).toBe(true);
    });
  });
});
