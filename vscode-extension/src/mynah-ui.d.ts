declare module "@aws/mynah-ui" {
  export type ChatItem = Record<string, unknown>;

  export const ChatItemType: Record<string, string> & {
    ANSWER_STREAM: string;
    ANSWER: string;
    PROMPT: string;
    SYSTEM_PROMPT: string;
  };

  export class MynahUI {
    constructor(config: Record<string, unknown>);

    addChatItem(tabId: string, item: ChatItem): void;
    updateChatAnswerWithMessageId(
      tabId: string,
      messageId: string,
      item: ChatItem,
    ): void;
    updateStore(tabId: string, update: Record<string, unknown>): string;
    getPromptInputText(tabId: string): string;
    getAllTabs(): Record<string, unknown>;
    getSelectedTabId(): string;
    updateLastChatAnswer(tabId: string, item: ChatItem): void;
    endMessageStream(tabId: string, messageId: string): void;
    addCustomContextToPrompt(
      tabId: string,
      contextItems: Array<Record<string, unknown>>,
    ): void;
  }
}
