import { Tab } from '../tabs';

describe('toggle (tabs)', () => {
  it('items', () => {
    const testToggle = new Tab({
      options: [
        {
          label: 'label1',
          value: 'value1',
        },
        {
          label: 'label2',
          value: 'value2',
        },
      ],
      direction: 'horizontal',
      name: 'testToggle',
      value: 'value2',
    });

    expect(testToggle.render.textContent).toContain('label1');
    expect(testToggle.render.textContent).toContain('label2');
    expect((testToggle.render.children[0].querySelector('input') as HTMLInputElement).getAttribute('checked')).toBeNull();
    // Second item should be currently selected
    expect((testToggle.render.children[1].querySelector('input') as HTMLInputElement).getAttribute('checked')).toBe('checked');
  });

  it('event handler', () => {
    const mockOnChangeHandler = jest.fn();
    const mockOnRemoveHandler = jest.fn();

    const testToggle = new Tab({
      options: [],
      direction: 'horizontal',
      name: 'testToggle',
      onChange: mockOnChangeHandler,
      onRemove: mockOnRemoveHandler,
    });

    testToggle.addOption({
      label: 'label1',
      value: 'value1',
    });
    testToggle.addOption({
      label: 'label2',
      value: 'value2',
    });
    testToggle.setValue('value2');

    // Try to click and select the first item
    document.body.appendChild(testToggle.render as HTMLElement);
    const firstItemElement = document.querySelector('span[key="testToggle-value1"]') as HTMLElement;
    (firstItemElement.querySelector('input') as HTMLInputElement).click();
    expect(mockOnChangeHandler).toHaveBeenCalledTimes(1);
    expect(mockOnChangeHandler).toHaveBeenCalledWith('value1');

    // Try to click the remove button on the second item
    const secondItemElement = document.querySelector('span[key="testToggle-value2"]') as HTMLInputElement;
    (secondItemElement.querySelector('button') as HTMLButtonElement).click();
    expect(mockOnRemoveHandler).toHaveBeenCalledTimes(1);
    expect(mockOnRemoveHandler).toHaveBeenCalledWith('value2', expect.anything());
  });
});
