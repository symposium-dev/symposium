import { Notification } from '../notification';

describe('notification', () => {
  it('notify', () => {
    const mockClickHandler = jest.fn();
    const mockCloseHandler = jest.fn();
    const testNotification = new Notification({
      title: 'test notification title',
      content: 'test notification content',
      onNotificationClick: mockClickHandler,
      onNotificationHide: mockCloseHandler,
    });
    testNotification.notify();
    const notificationElement: HTMLDivElement | null = document.body.querySelector('.mynah-notification');
    expect(notificationElement?.querySelector('h3')?.textContent).toBe('test notification title');
    expect(notificationElement?.querySelector('.mynah-notification-content')?.textContent).toBe('test notification content');
    notificationElement?.click();
    expect(mockClickHandler).toHaveBeenCalledTimes(1);
    expect(mockCloseHandler).toHaveBeenCalledTimes(1);
  });
});
