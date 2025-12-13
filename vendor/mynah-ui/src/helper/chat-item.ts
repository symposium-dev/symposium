import { ChatItem, ChatItemContent } from '../static';

export const emptyChatItemContent: ChatItemContent = {
  header: null,
  body: null,
  buttons: null,
  codeBlockActions: null,
  codeReference: null,
  customRenderer: null,
  fileList: null,
  followUp: null,
  summary: null,
  footer: null,
  formItems: null,
  informationCard: null,
  relatedContent: null,
  tabbedContent: null
};

export const chatItemHasContent = (chatItem: Partial<ChatItem>): boolean => (
  (chatItem.body != null && chatItem.body !== '') ||
  chatItem.fileList != null ||
  chatItem.formItems != null ||
  chatItem.header != null ||
  chatItem.footer != null ||
  chatItem.summary != null ||
  chatItem.customRenderer != null ||
  chatItem.informationCard != null ||
  chatItem.buttons != null);

export const copyToClipboard = async (
  textToSendClipboard: string,
  onCopied?: () => void
): Promise<void> => {
  if (!document.hasFocus?.()) {
    window.focus();
  }
  try {
    await navigator.clipboard.writeText(textToSendClipboard);
  } finally {
    onCopied?.();
  }
};
