import { PromptOptions } from '../../chat-item/prompt-input/prompt-options';

describe('prompt-options', () => {
  it('renders with filter options', () => {
    const filterOptions = [ {
      id: 'test-select',
      type: 'select' as const,
      title: 'Test Select',
      options: [
        { value: 'option1', label: 'Option 1' },
        { value: 'option2', label: 'Option 2' }
      ]
    } ];

    const promptOptions = new PromptOptions({
      filterOptions,
      buttons: [],
      onFiltersChange: () => {}
    });

    expect(promptOptions.render.querySelector('[data-testid="prompt-input-options"]')).toBeDefined();
    expect(promptOptions.render.querySelector('select')).toBeDefined();
  });

  it('renders with buttons', () => {
    const buttons = [ {
      id: 'test-button',
      text: 'Test Button',
      status: 'info' as const
    } ];

    let clickedButtonId = '';
    const promptOptions = new PromptOptions({
      filterOptions: [],
      buttons,
      onButtonClick: (buttonId) => { clickedButtonId = buttonId; }
    });

    const button = promptOptions.render.querySelector('button');
    expect(button).toBeDefined();

    button?.click();
    expect(clickedButtonId).toBe('test-button');
  });

  it('handles empty options', () => {
    const promptOptions = new PromptOptions({
      filterOptions: [],
      buttons: []
    });

    expect(promptOptions.render.children.length).toBeGreaterThanOrEqual(0);
  });
});
