import { Button } from '../button';

describe('button', () => {
  it('label', () => {
    const mockOnClickHandler = jest.fn();
    const testButton = new Button({
      label: 'Test button',
      onClick: mockOnClickHandler,
    });

    expect(testButton.render).toBeDefined();
    expect(testButton.render.querySelector('span')?.textContent).toBe('Test button');

    testButton.updateLabel('Updated label');
    expect(testButton.render.textContent).toBe('Updated label');
  });

  it('attributes', () => {
    const mockOnClickHandler = jest.fn();
    const testButton = new Button({
      label: 'Test button',
      attributes: {
        id: 'test-id',
      },
      onClick: mockOnClickHandler,
    });

    expect(testButton.render.id).toBe('test-id');
  });

  it('primary style', () => {
    const mockOnClickHandler = jest.fn();
    const testButton = new Button({
      label: 'Test button',
      primary: false,
      onClick: mockOnClickHandler,
    });
    const testButton2 = new Button({
      label: 'Test button',
      primary: true,
      onClick: mockOnClickHandler,
    });

    expect(testButton.render.classList.contains('mynah-button-secondary')).toBeTruthy();
    expect(testButton2.render.classList.contains('mynah-button-secondary')).toBeFalsy();
  });

  it('enabled', () => {
    const mockOnClickHandler = jest.fn();
    const testButton = new Button({
      label: 'Test button',
      onClick: mockOnClickHandler,
    });

    expect(testButton.render.disabled).toBeFalsy();
    testButton.setEnabled(false);
    expect(testButton.render.disabled).toBeTruthy();
  });

  it('event handlers', () => {
    const mockOnClickHandler = jest.fn();
    const mockMouseOverHandler = jest.fn();
    const testButton = new Button({
      label: 'Test button',
      attributes: {
        id: 'test-id',
      },
      onClick: mockOnClickHandler,
      additionalEvents: {
        mouseenter: mockMouseOverHandler,
      }
    });

    document.body.appendChild(testButton.render);
    const testButtonElement = document.querySelector('#test-id') as HTMLElement;
    testButtonElement?.click();
    expect(mockOnClickHandler).toHaveBeenCalledTimes(1);

    testButtonElement.dispatchEvent(new Event('mouseenter'));
    expect(mockMouseOverHandler).toHaveBeenCalledTimes(1);
  });
});
