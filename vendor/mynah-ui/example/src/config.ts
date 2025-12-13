import { ChatItemType, MynahIcons, ChatItemButton, MynahUITabStoreTab, QuickActionCommandGroup, TabBarMainAction } from '@aws/mynah-ui';
import { defaultFollowUps } from './samples/sample-data';
import { Commands } from './commands';
export const WelcomeMessage = `Hi, this is \`MynahUI\` and it is a **data and event driven** web based chat interface library and it is independent from any framework like react or vue etc.
In this example web app which uses mynah-ui as its renderer, we're simulating its capabilities with some static content with an IDE look&feel.

*To see more examples about the possible content types, interactions or various component types, you can type \`/\` to open the quick actions list panel.*
`;

export const mcpButton: TabBarMainAction = {
    id: 'mcp-init',
    description: 'Initializing MCP servers',
    icon: 'mcp',
};

export const rulesButton: ChatItemButton = {id: 'Rules', status: 'clear', text: 'Rules', icon: 'check-list'}

export const promptTopBarTitle = '@ Pin Context'

export const tabbarButtons: TabBarMainAction[] = [
    {
        id: 'clear',
        description: 'Clear messages in this tab',
        icon: MynahIcons.REFRESH,
    },
    {
        id: 'multi',
        icon: MynahIcons.ELLIPSIS,
        items: [
            {
                id: 'new-welcome-screen',
                text: 'Welcome screen',
                icon: MynahIcons.Q,
            },
            {
                id: 'account-details',
                text: 'Non chat tab (Account Details)',
                icon: MynahIcons.USER,
            },
            {
                id: 'splash-loader',
                text: 'Show splash loader',
                icon: MynahIcons.PAUSE,
            },
            {
                id: 'custom-data-check',
                text: 'Custom check',
                icon: MynahIcons.MAGIC,
            },
            {
                id: 'history_sheet',
                text: 'Sheet (History)',
                icon: MynahIcons.HISTORY,
            },
            {
                id: 'memory_sheet',
                text: 'Sheet (Memory)',
                icon: MynahIcons.COMMENT,
            },
            {
                id: 'show-avatars',
                text: 'Show/Hide avatars',
                icon: MynahIcons.USER,
            },
             {
                id: 'show-pinned-context',
                text: 'Show/Hide Pinned Context',
                icon: MynahIcons.PIN,
            },
            {
                id: 'show-code-diff',
                text: 'Show code diff!',
                icon: MynahIcons.CODE_BLOCK,
            },
            {
                id: 'insert-code',
                icon: MynahIcons.CURSOR_INSERT,
                text: 'Insert code!',
            },
            {
                id: 'save-session',
                icon: MynahIcons.DEPLOY,
                text: 'Save session',
            },
            {
                id: 'remove-saved-session',
                icon: MynahIcons.REVERT,
                text: 'Remove saved session',
            },
            {
                id: 'export-chat-md',
                icon: MynahIcons.EXTERNAL,
                text: 'Export chat (md)',
            },
            {
                id: 'export-chat-html',
                icon: MynahIcons.EXTERNAL,
                text: 'Export chat (html)',
            },
            {
                id: 'enable-disable-progress-bar',
                icon: MynahIcons.PLAY,
                text: 'Enable/disable Progress bar',
            },
            {
                id: 'animation-fast',
                icon: MynahIcons.FLASH,
                text: 'Fast Animation (100ms)',
            },
            {
                id: 'animation-normal',
                icon: MynahIcons.CALENDAR,
                text: 'Normal Animation (500ms)',
            },
            {
                id: 'animation-disabled',
                icon: MynahIcons.BLOCK,
                text: 'Disable Animation',
            },
        ],
    },
];
export const QuickActionCommands: QuickActionCommandGroup[] = [
    {
        groupName: 'Examples of **Prompt input field** items',
        commands: [
            {
                command: Commands.INSERT_CODE,
                icon: MynahIcons.CODE_BLOCK,
                description:
                    'Inserts a dummy code under the prompt field which will be sent together with the prompt text.',
            },
            {
                command: Commands.COMMAND_WITH_PROMPT,
                icon: MynahIcons.ASTERISK,
                placeholder: 'Enter your prompt',
                description:
                    'A quick action command which is not running immediately after it is selected which allows you to write an additional prompt text if you want.',
            },
            {
                command: '/disabled',
                icon: MynahIcons.BLOCK,
                disabled: true,
                description: 'This item is disabled for some reason',
            },
            {
                command: Commands.SHOW_STICKY_CARD,
                icon: MynahIcons.INFO,
                description:
                    'You can stick a ChatItem card on top of the input field which will stay there independently from the conversation block. It might be handy to give some info to the user.',
            },
        ],
    },
    {
        groupName: 'Examples of **ChatItems**',
        commands: [
            {
                command: Commands.STATUS_CARDS,
                icon: MynahIcons.WARNING,
                description:
                    'ChatItem cards can tell more with some status colors on borders together with icons. See different examples with status colors applied!',
            },
            {
                command: Commands.HEADER_TYPES,
                icon: MynahIcons.COMMENT,
                description: 'ChatItem cards with different headers, padding, fullWidth and directive types.',
            },
            {
                command: Commands.SUMMARY_CARD,
                icon: MynahIcons.RIGHT_OPEN,
                description: 'ChatItem card with summary field.',
            },
            {
                command: Commands.FORM_CARD,
                icon: MynahIcons.LIST_ADD,
                description: 'ChatItem cards can have forms inside, including several input items and buttons!',
            },
            {
                command: Commands.FILE_LIST_CARD,
                icon: MynahIcons.FOLDER,
                description:
                    'ChatItem cards can show a file list with a proper file-tree look. And those files can have actions and statuses with information too.',
            },
            {
                command: Commands.CARD_WITH_MARKDOWN_LIST,
                icon: MynahIcons.CHECK_LIST,
                description: 'ChatItem card with a complex markdown list inside.',
            },
            {
                command: Commands.CARD_WITH_PROGRESSIVE_FILE_LIST,
                icon: MynahIcons.FILE,
                description: 'ChatItem card with a progressive file list.',
            },
            {
                command: Commands.VOTE,
                icon: MynahIcons.THUMBS_UP,
                description: 'ChatItem card which can be voted',
            },
            {
                command: Commands.CARD_WITH_ALL_MARKDOWN_TAGS,
                icon: MynahIcons.HELP,
                description: 'ChatItem card with a markdown file with all markdown tags',
            },
            {
                command: Commands.CARD_RENDER_MARKDOWN_TABLE,
                icon: MynahIcons.TABS,
                description: 'ChatItem card for markdown table',
            },
            {
                command: Commands.CARD_SNAPS_TO_TOP,
                icon: MynahIcons.UP_OPEN,
                description:
                    'ChatItem card which snaps to top of the scolling container after the stream finishes or when the snapToTop value is set to true.',
            },
            {
                command: Commands.PROGRESSIVE_CARD,
                icon: MynahIcons.TRANSFORM,
                description:
                    "ChatItem cards can show a progress with its content. It doesn't have to be a stream by appending text each time.",
            },
            {
                command: Commands.IMAGE_IN_CARD,
                icon: MynahIcons.DOC,
                description:
                    'ChatItem cards can have various items which can be written with markdown and image is also one of them.',
            },
            {
                command: Commands.CUSTOM_RENDERER_CARDS,
                icon: MynahIcons.EXTERNAL,
                description:
                    'Struggling with markdown texts to produce a rich but static content which has to be done on the frontend client? Thinking about how to write html markups directly or even more better way? Custom renderers got your back!',
            },
            {
                command: Commands.FOLLOWUPS_AT_RIGHT,
                icon: MynahIcons.RIGHT_OPEN,
                description:
                    'You can set the position of the followups too. By simply setting the type of the ChatItem.',
            },
            {
                command: Commands.BORDERED_CARDS,
                icon: MynahIcons.INFO,
                description:
                    'ChatItem cards with border styling for important notifications or reroute messages.',
            },
        ],
    },
    {
        groupName: 'Examples of **system wide components**',
        commands: [
            {
                command: Commands.SHOW_CUSTOM_FORM,
                icon: MynahIcons.CHECK_LIST,
                description:
                    'Do you know the feedback from which appears when you downvote a card and click to "Report an issue"? You can use that block generate your custom forms too.',
            },
            {
                command: Commands.NOTIFY,
                icon: MynahIcons.MEGAPHONE,
                description: 'It will show you a notification',
            },
        ],
    },
    {
        groupName: 'Actions for the **Demo App**',
        commands: [
            {
                command: Commands.CLEAR,
                icon: MynahIcons.TRASH,
                description: 'Clears all the messages in this tab.',
            },
            {
                command: Commands.CLEAR_CONTEXT_ITEMS,
                icon: MynahIcons.TRASH,
                description: 'Clears all context items for this tab.',
            },
            {
                command: Commands.CLEAR_LOGS,
                icon: MynahIcons.CANCEL,
                description: 'Clears logs on the bottom left.',
            },
            {
                command: '/animation-demo',
                icon: MynahIcons.FLASH,
                description: 'Test streaming animation with different speeds. Use tab bar buttons to switch modes.',
            },
        ],
    },
];

export const mynahUIDefaults: Partial<MynahUITabStoreTab> = {
    store: {
        tabTitle: 'Chat',
        cancelButtonWhenLoading: true,
        promptInputInfo:
            'This is the information field. Check [MynahUI Data Model](https://github.com/aws/mynah-ui/blob/main/docs/DATAMODEL.md) for more details.',
        promptInputOptions: [
            {
                type: 'switch',
                id: 'pair-programmer-mode',
                tooltip: 'Disable pair programmer mode',
                alternateTooltip: 'Enable pair programmer mode',
                value: 'false',
                icon: 'code-block',
            },
            {
                type: 'select',
                border: false,
                autoWidth: true,
                id: 'model-select',
                mandatory: true,
                hideMandatoryIcon: true,
                options: [
                    {
                        label: 'Fast',
                        value: 'fast',
                        description: 'Quick responses with good accuracy'
                    },
                    {
                        label: 'Fast 2.0 (Exp.)',
                        value: 'fast-2-experimental',
                        description: 'Experimental faster model with enhanced capabilities'
                    },
                    {
                        label: 'Decisive',
                        value: 'decisive',
                        description: 'More thorough analysis with detailed responses'
                    },
                ]
            }
        ],
        promptInputButtons: [{
            id: 'test-prompt-input-button',
            icon: 'bug',
            description: 'HellO!',
            status: 'clear'
        }],
        chatItems: [
            {
                type: ChatItemType.ANSWER,
                body: WelcomeMessage,
                footer: {
                    fileList: {
                        rootFolderTitle: undefined,
                        fileTreeTitle: '',
                        filePaths: ['./src/index.ts'],
                        details: {
                            './src/index.ts': {
                                icon: MynahIcons.FILE,
                                clickable: false,
                                description: `Files used for this response: **index.ts**
        Use \`@\` to mention a file, folder, or method.`,
                            },
                        },
                    },
                },
                messageId: 'welcome-message',
                followUp: defaultFollowUps.followUp,
            },
        ],
        quickActionCommands: QuickActionCommands,
        contextCommands: [
            {
                commands: [
                    {
                        command: '@workspace',
                        placeholder: 'Yes, you selected workspace :P',
                        description: 'Reference all code in workspace.',
                    },
                    {
                        command: 'image',
                        icon: MynahIcons.IMAGE,
                        description: 'Add an image to the context',
                        placeholder: 'Select an image file'
                    },
                    {
                        command: 'folder',
                        icon: MynahIcons.FOLDER,
                        children: [
                            {
                                groupName: 'Folders',
                                commands: [
                                    {
                                        command: 'src',
                                        icon: MynahIcons.FOLDER,
                                        children: [
                                            {
                                                groupName: 'src/',
                                                commands: [
                                                    {
                                                        command: 'index.ts',
                                                        icon: MynahIcons.FILE,
                                                    },
                                                ],
                                            },
                                        ],
                                    },
                                    {
                                        command: 'main',
                                        description: './src/',
                                        icon: MynahIcons.FOLDER,
                                    },
                                    {
                                        command: 'components',
                                        description: './src/',
                                        icon: MynahIcons.FOLDER,
                                    },
                                    {
                                        command: 'helper',
                                        description: './src/',
                                        icon: MynahIcons.FOLDER,
                                    },
                                    {
                                        command: 'src',
                                        description: './example/',
                                        icon: MynahIcons.FOLDER,
                                    },
                                ],
                            },
                        ],
                        placeholder: 'Mention a specific folder',
                        description: 'All files within a specific folder',
                    },
                    {
                        command: 'file',
                        icon: MynahIcons.FILE,
                        children: [
                            {
                                groupName: 'Files',
                                commands: [
                                    {
                                        command: 'monarch.ts',
                                        description: 'spring-boot-template/.github/workflows/p-openapi.yaml',
                                        icon: MynahIcons.FILE,
                                    },
                                    {
                                        command: 'main.ts',
                                        description: './src/',
                                        icon: MynahIcons.FILE,
                                    },
                                    {
                                        command: 'button.ts',
                                        description: './src/components/',
                                        icon: MynahIcons.FILE,
                                    },
                                    {
                                        command: 'ex-dom.ts',
                                        description: './src/helper/',
                                        icon: MynahIcons.FILE,
                                    },
                                    {
                                        command: 'dom.ts',
                                        description: './src/helper/',
                                        icon: MynahIcons.FILE,
                                    },
                                    {
                                        command: '_dark.scss',
                                        description: './src/styles/',
                                        icon: MynahIcons.FILE,
                                        // add route just to check if it returns back
                                        route: ['src', 'styles'],
                                    },
                                ],
                            },
                        ],
                        placeholder: 'Mention a specific file',
                        description: 'Add a file to context',
                    },
                    {
                        command: 'symbols',
                        icon: MynahIcons.CODE_BLOCK,
                        children: [
                            {
                                groupName: 'Symbols',
                                commands: [
                                    {
                                        command: 'DomBuilder',
                                        icon: MynahIcons.CODE_BLOCK,
                                        description: 'The DomGeneration function in dom.ts file',
                                    },
                                    ...Array(100_000)
                                        .fill(null)
                                        .map((_, i) => ({
                                            command: `item${i}`,
                                            description: `./src/${i}`,
                                            icon: MynahIcons.CODE_BLOCK,
                                        })),
                                ],
                            },
                        ],
                        placeholder: 'Select a symbol',
                        description: 'After that mention a specific file/folder, or leave blank for full project!',
                    },
                    {
                        command: 'prompts',
                        icon: MynahIcons.FLASH,
                        description: 'Saved prompts, to reuse them in your current prompt',
                        children: [
                            {
                                groupName: 'Prompts',
                                actions: [
                                    {
                                        id: 'add-new-prompt',
                                        icon: 'plus',
                                        text: 'Add',
                                        description: 'Add new prompt',
                                    },
                                ],
                                commands: [
                                    {
                                        command: 'python_expert',
                                        icon: MynahIcons.CHAT,
                                        description: 'Expert on python stuff',
                                    },
                                    {
                                        command: 'javascript_expert',
                                        icon: MynahIcons.CHAT,
                                        description: 'Expert on Javascript and typescript',
                                    },
                                    {
                                        command: 'Add Prompt',
                                        icon: MynahIcons.PLUS,
                                    },
                                ],
                            },
                        ],
                    },
                    {
                        command: 'disabled-example',
                        icon: MynahIcons.BLOCK,
                        description: 'Example of disabled item with custom text',
                        disabled: true,
                        disabledText: 'pending'
                    },
                ],
            },
        ],
        promptInputPlaceholder: 'Type something or "/" for quick action commands or @ for choosing context',
        tabBarButtons: [mcpButton, ...tabbarButtons],
        promptTopBarTitle: promptTopBarTitle,
        promptTopBarButton: rulesButton,
    },
};
