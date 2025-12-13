import {
  ChatItemType,
  MynahUITabStoreTab,
  QuickActionCommandGroup,
  MynahIcons
} from '@aws/mynah-ui';
import { Commands, mockFollowups, welcomeMessage } from './mocks/mock-data';

export const quickActionCommands: QuickActionCommandGroup[] = [
  {
    commands: [
      {
        command: Commands.HELP,
        description: 'Show help text.',
      },
      {
        command: Commands.CLEAR,
        description: 'Clear all chat.',
      }
    ],
  },
];

export const contextCommands: QuickActionCommandGroup[] = [
  {
    groupName: 'Mention code',
    commands: [
      {
        command: '@workspace',
        description: 'Reference all code in workspace.'
      },
      {
        command: '@file',
        description: 'Reference the code in the current file.'
      }
    ]
  }
];

export const defaultDataSet: Partial<MynahUITabStoreTab> = {
  store: {
    tabTitle: 'Chat',
    cancelButtonWhenLoading: true,
    promptInputInfo: 'This is the **footer** of the panel.',
    chatItems: [
      {
        type: ChatItemType.ANSWER,
        body: welcomeMessage,
        messageId: 'welcome-message'
      },
      mockFollowups
    ],
    quickActionCommands,
    quickActionCommandsHeader: {
      status: 'warning',
      icon: MynahIcons.INFO,
      title: 'Q Developer agentic capabilities',
      description: 'You can now ask Q directly in the chat to generate code, documentation, and unit tests. You don\'t need to explicitly use /dev, /test, or /doc',
    },
    contextCommands,
    promptInputPlaceholder: 'Type something or "/" for quick action commands or @ for choosing context',
  }
};
