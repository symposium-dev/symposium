import { ChatPromptInputCommand } from '../../chat-item/chat-prompt-input-command';

describe('chat-prompt-input-command', () => {
  it('renders with command text', () => {
    const command = new ChatPromptInputCommand({
      command: 'test-command',
      onRemoveClick: () => {}
    });

    expect(command.render.querySelector('.mynah-chat-prompt-input-command-text')).toBeDefined();
    expect(command.render.classList.contains('hidden')).toBe(false);
  });

  it('handles empty command', () => {
    const command = new ChatPromptInputCommand({
      command: '',
      onRemoveClick: () => {}
    });

    expect(command.render.classList.contains('hidden')).toBe(true);
  });

  it('handles remove click', () => {
    let removeClicked = false;
    const command = new ChatPromptInputCommand({
      command: 'test-command',
      onRemoveClick: () => { removeClicked = true; }
    });

    const textElement = command.render.querySelector('.mynah-chat-prompt-input-command-text') as HTMLElement;
    textElement.click();
    expect(removeClicked).toBe(true);
  });

  it('sets command text', () => {
    const command = new ChatPromptInputCommand({
      command: 'initial',
      onRemoveClick: () => {}
    });

    command.setCommand('new-command');
    expect(command.render.classList.contains('hidden')).toBe(false);

    command.setCommand('');
    expect(command.render.classList.contains('hidden')).toBe(true);
  });
});
