/* eslint-disable @typescript-eslint/no-dynamic-delete */
import { Connector, INITIAL_STREAM_DELAY } from './connector';
import {
    MynahUI,
    MynahUIDataModel,
    ChatPrompt,
    RelevancyVoteType,
    ChatItemType,
    FeedbackPayload,
    ChatItemAction,
    NotificationType,
    ChatItem,
    MynahIcons,
    generateUID,
    KeyMap,
    TreeNodeDetails,
    QuickActionCommand,
    ChatItemButton,
    CustomQuickActionCommand,
    DropdownListOption
} from '@aws/mynah-ui';
import { mcpButton, mynahUIDefaults, promptTopBarTitle, rulesButton, tabbarButtons } from './config';
import { Log, LogClear } from './logger';
import {
    exampleCodeBlockToInsert,
    exampleCustomRendererWithHTMLMarkup,
    exampleCustomRendererWithDomBuilderJson,
    exampleFileListChatItem,
    exampleFileListChatItemForUpdate,
    defaultFollowUps,
    exampleFormChatItem,
    exampleImageCard,
    exampleProgressCards,
    exampleRichFollowups,
    exampleStreamParts,
    sampleMarkdownList,
    exampleCodeDiff,
    exampleCodeDiffApplied,
    sampleAllInOneList,
    sampleTableList,
    exampleInformationCard,
    exampleBorderedCard,
    exploreTabData,
    qAgentQuickActions,
    welcomeScreenTabData,
    exampleConfirmationButtons,
    exampleButtons,
    exampleStatusButtons,
    exampleVoteChatItem,
    sampleHeaderTypes,
    sampleProgressiveFileList,
    sampleMCPList,
    sampleMCPDetails,
    mcpToolRunSampleCard,
    mcpToolRunSampleCardInit,
    sampleRulesList,
    accountDetailsTabData,
} from './samples/sample-data';
import escapeHTML from 'escape-html';
import './styles/styles.scss';
import { ThemeBuilder } from './theme-builder/theme-builder';
import { Commands } from './commands';

export const createMynahUI = (initialData?: MynahUIDataModel): MynahUI => {
    const connector = new Connector();
    let streamingMessageId: string | null;
    let showChatAvatars: boolean = false;
    let showPinnedContext: boolean = true;

    // Get animation config from URL hash
    const getAnimationConfig = () => {
        const hash = window.location.hash.replace('#', '');
        switch (hash) {
            case 'fast-animation':
                return {
                    typewriterStackTime: 100,
                    typewriterMaxWordTime: 20,
                };
            case 'normal-animation':
                return {
                    typewriterStackTime: 500,
                    typewriterMaxWordTime: 50,
                };
            case 'no-animation':
                return {
                    disableTypewriterAnimation: true,
                };
            default:
                return {};
        }
    };

    const mynahUI = new MynahUI({
        loadStyles: true,
        splashScreenInitialStatus: {
            visible: true,
            text: 'Initializing',
        },
        rootSelector: '#amzn-mynah-website-wrapper',
        defaults: {
            store: {
                ...mynahUIDefaults.store,
                showChatAvatars,
            },
        },
        config: {
            maxTabs: 5,
            maxTabsTooltipDuration: 5000,
            noMoreTabsTooltip: 'You can only open five conversation tabs at a time.',
            autoFocus: true,
            dragOverlayIcon: MynahIcons.IMAGE,
            ...getAnimationConfig(),
            texts: {
                dragOverlayText: 'Add Image to Context',
                stopGeneratingTooltip: 'Stop &#8984; Backspace',
                feedbackFormDescription:
                    '_Feedback is anonymous. For issue updates, please contact us on [GitHub](https://github.com/aws/mynah-ui/issues)._',
            },
        },
        tabs: JSON.parse(localStorage.getItem('mynah-ui-storage') as string) ?? {
            'tab-1': {
                isSelected: true,
                store: {
                    ...mynahUIDefaults.store,
                    ...welcomeScreenTabData.store,
                },
            },
        },
        onPromptInputOptionChange: (tabId, optionsValues) => {
            if (optionsValues['pair-programmer-mode'] != null) {
                mynahUI.addChatItem(tabId, {
                    type: ChatItemType.DIRECTIVE,
                    contentHorizontalAlignment: 'center',
                    fullWidth: true,
                    body: `
Pair programmer mode - ${optionsValues['pair-programmer-mode'] === 'true' ? 'ON' : 'OFF'}
Model - ${optionsValues['model-select'] !== '' ? optionsValues['model-select'] : 'auto'}
`,
                });
            }
            Log(`Prompt options change for tab <b>${tabId}</b>:<br/>
        ${
            optionsValues
                ? `<br/>Options:<br/>${Object.keys(optionsValues)
                      .map((optionId) => {
                          return `<b>${optionId}</b>: ${(optionsValues as Record<string, string>)[optionId] ?? ''}`;
                      })
                      .join('<br/>')}`
                : ''
        }
        `);
        },
        onSplashLoaderActionClick: (action) => {
            Log(`Splash loader action click <b>${action.id}</b>`);
            mynahUI.toggleSplashLoader(false);
            if (action.id === 'cancel-mcp-save') {
                mynahUI.notify({
                    duration: 3000,
                    content: 'MCP save',
                    title: 'Saving operation is cancelled.',
                    type: NotificationType.ERROR
                });
            } else if (action.id === 'hide-mcp-save') {
                mynahUI.notify({
                    duration: 3000,
                    content: 'MCP save',
                    title: 'Save progress goes on in the background',
                    type: NotificationType.INFO
                });
            }
        },
        onDropDownOptionChange: (tabId: string, messageId: string, value: DropdownListOption []) => {
            Log(`Dropdown Option changed in message ${messageId} on tab ${tabId}`)
        },
        onDropDownLinkClick: (tabId, actionId, destination) => {
            Log(`Dropdown link click with id ${tabId}, ${actionId}, ${destination}`)
        },
        onPromptInputButtonClick: (tabId, buttonId) => {
            Log(`Prompt input button ${buttonId} clicked on tab <b>${tabId}</b>`);
        },
        onFocusStateChanged: (focusState: boolean) => {
            Log(`MynahUI focus state changed: <b>${focusState.toString()}</b>`);
        },
        onPromptTopBarItemAdded: (tabId: string, item: QuickActionCommand) => {
                                        Log(`Prompt top bar item <b>${item.command}</b> added on tab <b>${tabId}</b>`);

            mynahUI.updateStore(tabId, {
                                    promptTopBarContextItems: [
                                        ...((mynahUI.getTabData(tabId).getValue('promptTopBarContextItems') as QuickActionCommand[]).filter((existingItem) => existingItem.command !== item.command) ?? []),
                                        item,
                                    ],
                                });
        },
        onPromptTopBarItemRemoved: (tabId: string, item: QuickActionCommand) => {
                                        Log(`Prompt top bar item <b>${item.command}</b> removed on tab <b>${tabId}</b>`);

            mynahUI.updateStore(tabId, {
                                    promptTopBarContextItems:(mynahUI.getTabData(tabId).getValue('promptTopBarContextItems') as QuickActionCommand[]).filter((existingItem) => existingItem.command !== item.command)
                                });
        },
        onPromptTopBarButtonClick: (tabId: string, button: ChatItemButton) => {
                                        Log(`Top bar button <b>${button.id}</b> clicked on tab <b>${tabId}</b>`);

         const topBarOverlay =   mynahUI.openTopBarButtonOverlay({tabId, topBarButtonOverlay: sampleRulesList,
            events: {
            onClose: () => {Log(`Top bar overlay closed on tab <b>${tabId}</b>`)},
            onGroupClick: (group) => {Log(`Top bar overlay group clicked <b>${group}</b> on tab <b>${tabId}</b>`)},
            onItemClick: (item) => {  Log(`Top bar overlay item clicked <b>${item.id}</b> on tab <b>${tabId}</b>`); topBarOverlay.update(sampleRulesList)},
            onKeyPress: (e) => {Log(`Key pressed on top bar overlay`);      if (e.key === KeyMap.ESCAPE) {
                                topBarOverlay.close();
                            }}

        }})


        },
        onTabBarButtonClick: (tabId: string, buttonId: string) => {
            if (buttonId.match('mcp-')) {
                if (buttonId === 'mcp-init') {
                    mcpButton.description = `No MCP servers.

  Click to configure.`;
                    mcpButton.id = 'mcp-no-server';
                } else if (buttonId === 'mcp-no-server') {
                    mcpButton.id = 'mcp-ok';
                    mcpButton.description = `MCP servers:
  0/2 initialized
  
  fetch-mcp: uvz: command not found
  
  jira: authorization failed
  
  Click to configure.`;
                }

                Object.keys(mynahUI.getAllTabs()).forEach((tabIdKey) => {
                    mynahUI.updateStore(tabIdKey, {
                        tabBarButtons: [mcpButton, ...tabbarButtons],
                    });
                });
                mynahUI.updateTabDefaults({
                    store: {
                        tabBarButtons: [mcpButton, ...tabbarButtons],
                    },
                });

                const mcpSheet = mynahUI.openDetailedList({
                    detailedList: sampleMCPList,
                    events: {
                        onFilterValueChange: (filterValues: Record<string, any>, isValid: boolean) => {
                            Log('Filter changed');
                        },
                        onFilterActionClick: (action, filterValues?: Record<string, any>, isValid?: boolean) => {
                            Log(`Filter action clicked: <b>${action.id}</b>`);
                            Log(`Filters: <b>${JSON.stringify(filterValues ?? {})}</b>`);
                            if (action.id === 'cancel-mcp') {
                                mcpSheet.update(sampleMCPList, false);
                            } else if (action.id === 'save-mcp') {
                                mynahUI.toggleSplashLoader(true, 'Saving **the MCP**', [
                                    {
                                        id: 'hide-mcp-save',
                                        status: 'clear',
                                        text: 'Hide',
                                    },
                                    {
                                        id: 'cancel-mcp-save',
                                        status: 'primary',
                                        text: 'Cancel',
                                    },
                                ]);
                                setTimeout(() => {
                                    mynahUI.toggleSplashLoader(false);
                                    mcpSheet.update(sampleMCPList, false);
                                }, 5000);
                            }
                        },
                        onKeyPress: (e) => {
                            Log('Key pressed');
                            if (e.key === KeyMap.ESCAPE) {
                                close();
                            }
                        },
                        onItemSelect: (detailedListItem) => {
                            Log('Item selected');
                        },
                        onItemClick: (detailedListItem) => {
                            Log(`Item clicked: <b>${detailedListItem.name}</b>`);
                            mcpSheet.update(sampleMCPDetails(detailedListItem.title ?? ''), true);
                        },
                        onActionClick: (button, detailedListItem) => {
                            if (button.id === 'open-mcp-xx') {
                                mcpSheet.update(sampleMCPDetails(detailedListItem?.title ?? ''), true);
                            }
                            Log('Action clicked');
                        },
                        onClose: () => {
                            Log('Sheet closed');
                        },
                        onTitleActionClick: (button) => {
                            if (button.id === 'back-to-mcp-list') {
                                mcpSheet.update(sampleMCPList);
                            }
                            if (button.id === 'mcp-delete-tool') {
                                mcpSheet.update(sampleMCPList, false);
                            }
                        },
                        onBackClick: () => {
                            mcpSheet.update(sampleMCPList, false);
                        },
                    },
                });
            } else if (buttonId === 'clear') {
                mynahUI.updateStore(tabId, {
                    chatItems: [],
                });
            } else if (buttonId === 'show-code-diff') {
                mynahUI.addChatItem(tabId, {
                    type: ChatItemType.ANSWER,
                    body: exampleCodeDiff,
                    codeBlockActions: {
                        copy: undefined,
                        'accept-diff': {
                            id: 'accept-diff',
                            label: 'Accept Diff',
                            flash: 'infinite',
                            icon: MynahIcons.OK_CIRCLED,
                            acceptedLanguages: ['diff-typescript'],
                            data: {
                                updatedCode: exampleCodeDiffApplied,
                            },
                        },
                    },
                });
                mynahUI.addChatItem(tabId, defaultFollowUps);
            } else if (buttonId === 'insert-code') {
                mynahUI.addToUserPrompt(tabId, exampleCodeBlockToInsert, 'code');
            } else if (buttonId === 'show-avatars') {
                showChatAvatars = !showChatAvatars;
                Object.keys(mynahUI.getAllTabs()).forEach((tabIdFromStore) =>
                    mynahUI.updateStore(tabIdFromStore, {
                        showChatAvatars: showChatAvatars,
                    }),
                );
            } else if (buttonId === 'show-pinned-context') {
                showPinnedContext = !showPinnedContext;
                if (showPinnedContext){
                Object.keys(mynahUI.getAllTabs()).forEach((tabIdFromStore) =>
                    mynahUI.updateStore(tabIdFromStore, {
                        promptTopBarTitle: promptTopBarTitle,
                        promptTopBarButton: rulesButton,
                    }),
                ); } else {
                         Object.keys(mynahUI.getAllTabs()).forEach((tabIdFromStore) =>
                    mynahUI.updateStore(tabIdFromStore, {
                        promptTopBarTitle: ``,
                    }),
                )
                }
            } else if (buttonId === 'splash-loader') {
                mynahUI.toggleSplashLoader(true, 'Showing splash loader...');
                setTimeout(() => {
                    mynahUI.toggleSplashLoader(false);
                }, 5000);
            } else if (buttonId === 'custom-data-check') {
                // Use for custom temporary checks
                mynahUI.addChatItem(tabId, {
                    type: ChatItemType.ANSWER,
                    header: {
                        icon: 'tools',
                        body: '#### Allow read-only tools outside your workspace',
                        status: {
                            icon: 'warning',
                            status: 'warning',
                        },
                        buttons: [
                            {
                                id: 'allow-readonly-tools',
                                text: 'Allow',
                                icon: 'ok',
                                status: 'clear',
                            },
                        ],
                    },
                    fullWidth: true,
                    body: '<span style="color:var(--mynah-color-text-weak)">Tools: tool-long-name-1, tool-long-name-2</span>',
                });
                mynahUI.addChatItem(tabId, {
                    type: ChatItemType.ANSWER,
                    title: 'SAVE THE DATE',
                    header: {
                        icon: 'calendar',
                        iconStatus: 'primary',
                        body: '## Soon, a new version will be released!',
                    },
                    fullWidth: true,
                    canBeDismissed: true,
                    body: "We're improving the performance, adding new features or making new UX changes every week. Save the date for new updates!.",
                });
                mynahUI.addChatItem(tabId, {
                    type: ChatItemType.ANSWER,
                    fullWidth: true,
                    buttons: [
                        {
                            id: 'accept-all-changed',
                            icon: 'ok',
                            position: 'outside',
                            text: 'Accept all',
                        },
                    ],
                });
                mynahUI.addChatItem(tabId, {
                    type: ChatItemType.ANSWER,
                    messageId: new Date().getTime().toString(),
                    body: 'Thinking...',
                    shimmer: true,
                });
            } else if (buttonId === 'history_sheet') {
                const { update, close, changeTarget, getTargetElementId } = mynahUI.openDetailedList({
                    detailedList: {
                        header: {
                            title: 'Chat history',
                        },
                        selectable: true,
                        filterOptions: [
                            {
                                type: 'textinput',
                                icon: MynahIcons.SEARCH,
                                id: generateUID(),
                                placeholder: 'Search...',
                                autoFocus: true,
                            },
                        ],
                        list: [
                            {
                                groupName: 'Today',
                                children: [
                                    {
                                        id: generateUID(),
                                        icon: MynahIcons.CHAT,
                                        description: 'Why is this unit test failing?',
                                        actions: [
                                            {
                                                id: generateUID(),
                                                icon: MynahIcons.EXTERNAL,
                                                text: 'Export',
                                            },
                                            {
                                                id: generateUID(),
                                                icon: MynahIcons.TRASH,
                                                text: 'Delete',
                                            },
                                        ],
                                    },
                                    {
                                        id: generateUID(),
                                        icon: MynahIcons.CHAT,
                                        description:
                                            '**Can you explain this error message in more detail? ArrayIndexOutOfBoundsException: 10 at Main.main(Main.java:4)**',
                                        actions: [
                                            {
                                                id: generateUID(),
                                                icon: MynahIcons.EXTERNAL,
                                                text: 'Export',
                                            },
                                            {
                                                id: generateUID(),
                                                icon: MynahIcons.TRASH,
                                                text: 'Delete',
                                            },
                                        ],
                                    },
                                    {
                                        id: generateUID(),
                                        icon: MynahIcons.CHECK_LIST,
                                        description: `some very long markdown string goes
here to see if it gets cut off properly as expected, with an ellipsis through css.some very long markdown string goes here to see if it gets cut off properly as expected, with an ellipsis through css.some very long markdown string goes here to see if it gets cut off properly as expected, with an ellipsis through css. some very long markdown string goes here to see if it gets cut off properly as expected, with an ellipsis through css. some very long markdown string goes here to see if it gets cut off properly as expected, with an ellipsis through css. some very long markdown string goes here to see if it gets cut off properly as expected, with an ellipsis through css.`,
                                        actions: [
                                            {
                                                id: generateUID(),
                                                icon: MynahIcons.EXTERNAL,
                                                text: 'Export',
                                            },
                                            {
                                                id: generateUID(),
                                                icon: MynahIcons.TRASH,
                                                text: 'Delete',
                                            },
                                        ],
                                    },
                                ],
                            },
                            {
                                groupName: 'Yesterday',
                                children: [
                                    {
                                        id: generateUID(),
                                        icon: MynahIcons.CHAT,
                                        description: 'How can I optimize utils.py for better performance?',
                                        actions: [
                                            {
                                                id: generateUID(),
                                                icon: MynahIcons.EXTERNAL,
                                                text: 'Export',
                                            },
                                            {
                                                id: generateUID(),
                                                icon: MynahIcons.TRASH,
                                                text: 'Delete',
                                            },
                                        ],
                                    },
                                    {
                                        id: generateUID(),
                                        icon: MynahIcons.CODE_BLOCK,
                                        description:
                                            '/dev Create a new REST API endpoint /api/authenticate to handle user authentication',
                                        actions: [
                                            {
                                                id: generateUID(),
                                                icon: MynahIcons.EXTERNAL,
                                                text: 'Export',
                                            },
                                            {
                                                id: generateUID(),
                                                icon: MynahIcons.TRASH,
                                                text: 'Delete',
                                            },
                                        ],
                                    },
                                    {
                                        id: generateUID(),
                                        icon: MynahIcons.CHAT,
                                        description:
                                            '**@workspace provide a refactored version of the endpoint() function**',
                                        actions: [
                                            {
                                                id: generateUID(),
                                                icon: MynahIcons.EXTERNAL,
                                                text: 'Export',
                                            },
                                            {
                                                id: generateUID(),
                                                icon: MynahIcons.TRASH,
                                                text: 'Delete',
                                            },
                                        ],
                                    },
                                    {
                                        id: generateUID(),
                                        icon: MynahIcons.CHAT,
                                        description: 'Explain the code in the mcp directory',
                                        actions: [
                                            {
                                                id: generateUID(),
                                                icon: MynahIcons.EXTERNAL,
                                                text: 'Export',
                                            },
                                            {
                                                id: generateUID(),
                                                icon: MynahIcons.TRASH,
                                                text: 'Delete',
                                            },
                                        ],
                                    },
                                ],
                            },
                            {
                                groupName: '4 days ago',
                                children: [
                                    {
                                        id: generateUID(),
                                        icon: MynahIcons.CHAT,
                                        description: 'What are the dependencies of this module?',
                                        actions: [
                                            {
                                                id: generateUID(),
                                                icon: MynahIcons.EXTERNAL,
                                                text: 'Export',
                                            },
                                            {
                                                id: generateUID(),
                                                icon: MynahIcons.TRASH,
                                                text: 'Delete',
                                            },
                                        ],
                                    },
                                    {
                                        id: generateUID(),
                                        icon: MynahIcons.CODE_BLOCK,
                                        description: '/dev Update CSS styles for responsive layout',
                                        actions: [
                                            {
                                                id: generateUID(),
                                                icon: MynahIcons.EXTERNAL,
                                                text: 'Export',
                                            },
                                            {
                                                id: generateUID(),
                                                icon: MynahIcons.TRASH,
                                                text: 'Delete',
                                            },
                                        ],
                                    },
                                ],
                            },
                            {
                                groupName: 'Last week',
                                children: [
                                    {
                                        id: generateUID(),
                                        icon: MynahIcons.CODE_BLOCK,
                                        description: '**/dev Optimize image loading for faster page loads**',
                                        actions: [
                                            {
                                                id: generateUID(),
                                                icon: MynahIcons.EXTERNAL,
                                                text: 'Export',
                                            },
                                            {
                                                id: generateUID(),
                                                icon: MynahIcons.TRASH,
                                                text: 'Delete',
                                            },
                                        ],
                                    },
                                    {
                                        id: generateUID(),
                                        icon: MynahIcons.CHAT,
                                        description:
                                            'What are some alternatives to generating a unique salt value in encrypt()?',
                                        actions: [
                                            {
                                                id: generateUID(),
                                                icon: MynahIcons.EXTERNAL,
                                                text: 'Export',
                                            },
                                            {
                                                id: generateUID(),
                                                icon: MynahIcons.TRASH,
                                                text: 'Delete',
                                            },
                                        ],
                                    },
                                    {
                                        id: generateUID(),
                                        icon: MynahIcons.CHAT,
                                        description:
                                            '**Generate a regular expression pattern that matches email addresses**',
                                        actions: [
                                            {
                                                id: generateUID(),
                                                icon: MynahIcons.EXTERNAL,
                                                text: 'Export',
                                            },
                                            {
                                                id: generateUID(),
                                                icon: MynahIcons.TRASH,
                                                text: 'Delete',
                                            },
                                        ],
                                    },
                                    {
                                        id: generateUID(),
                                        icon: MynahIcons.CHAT,
                                        description: 'Convert the selected code snippet to typescript',
                                        actions: [
                                            {
                                                id: generateUID(),
                                                icon: MynahIcons.EXTERNAL,
                                                text: 'Export',
                                            },
                                            {
                                                id: generateUID(),
                                                icon: MynahIcons.TRASH,
                                                text: 'Delete',
                                            },
                                        ],
                                    },
                                    {
                                        id: generateUID(),
                                        icon: MynahIcons.CHAT,
                                        description: 'Rewrite this sort function to use the merge sort algorithm',
                                        actions: [
                                            {
                                                id: generateUID(),
                                                icon: MynahIcons.EXTERNAL,
                                                text: 'Export',
                                            },
                                            {
                                                id: generateUID(),
                                                icon: MynahIcons.TRASH,
                                                text: 'Delete',
                                            },
                                        ],
                                    },
                                ],
                            },
                        ],
                    },
                    events: {
                        onFilterValueChange: (filterValues: Record<string, any>, isValid: boolean) => {
                            Log('Filter changed');
                        },
                        onKeyPress: (e) => {
                            Log('Key pressed');
                            if (e.key === KeyMap.ESCAPE) {
                                close();
                            } else if (e.key === KeyMap.ARROW_UP) {
                                changeTarget('up', true);
                            } else if (e.key === KeyMap.ARROW_DOWN) {
                                changeTarget('down', true);
                            } else if (e.key === KeyMap.ENTER) {
                                Log('Selected item with id: ' + getTargetElementId());
                            }
                        },
                        onItemSelect: (detailedListItem) => {
                            Log('Item selected');
                        },
                        onActionClick: (button) => {
                            Log('Action clicked');
                        },
                        onClose: () => {
                            Log('Sheet closed');
                        },
                    },
                });
            } else if (buttonId === 'memory_sheet') {
                const { close, update, changeTarget } = mynahUI.openDetailedList({
                    detailedList: {
                        header: {
                            title: 'Memories (16)',
                        },
                        textDirection: 'column',
                        selectable: false,
                        filterOptions: [
                            {
                                type: 'textinput',
                                icon: MynahIcons.SEARCH,
                                id: generateUID(),
                                placeholder: 'Search...',
                                autoFocus: true,
                            },
                            {
                                type: 'select',
                                id: generateUID(),
                                icon: MynahIcons.CHECK_LIST,
                                placeholder: 'All memories',
                                options: [
                                    {
                                        label: 'Created by user',
                                        value: 'user',
                                    },
                                    {
                                        label: 'Inferred by Q',
                                        value: 'q',
                                    },
                                ],
                            },
                        ],
                        list: [
                            {
                                groupName: 'Today',
                                children: [
                                    {
                                        title: 'Always add comments to my lines of Rust',
                                        description: 'Created by *user* at **2:45pm** on 1/2/24',
                                        actions: [
                                            {
                                                id: generateUID(),
                                                icon: MynahIcons.PENCIL,
                                                text: 'Edit',
                                            },
                                            {
                                                id: generateUID(),
                                                icon: MynahIcons.TRASH,
                                                text: 'Delete',
                                            },
                                        ],
                                    },
                                    {
                                        title: 'Always add comments to my lines of Rust',
                                        description: 'Created by user at **2:45pm** on 1/2/24',
                                        actions: [
                                            {
                                                id: generateUID(),
                                                icon: MynahIcons.PENCIL,
                                                text: 'Edit',
                                            },
                                            {
                                                id: generateUID(),
                                                icon: MynahIcons.TRASH,
                                                text: 'Delete',
                                            },
                                        ],
                                    },
                                ],
                            },
                            {
                                groupName: 'Yesterday',
                                children: [
                                    {
                                        title: 'Another memory',
                                        description: 'Inferred by Q at 2:45pm on 1/2/24',
                                        actions: [
                                            {
                                                id: generateUID(),
                                                icon: MynahIcons.PENCIL,
                                                text: 'Edit',
                                            },
                                            {
                                                id: generateUID(),
                                                icon: MynahIcons.TRASH,
                                                text: 'Delete',
                                            },
                                        ],
                                    },
                                    {
                                        title: 'Another memory',
                                        description: 'Inferred by Q at 2:45pm on 1/2/24',
                                        actions: [
                                            {
                                                id: generateUID(),
                                                icon: MynahIcons.PENCIL,
                                                text: 'Edit',
                                            },
                                            {
                                                id: generateUID(),
                                                icon: MynahIcons.TRASH,
                                                text: 'Delete',
                                            },
                                        ],
                                    },
                                    {
                                        title: 'Another memory',
                                        description: 'Inferred by Q at 2:45pm on 1/2/24',
                                        actions: [
                                            {
                                                id: generateUID(),
                                                icon: MynahIcons.PENCIL,
                                                text: 'Edit',
                                            },
                                            {
                                                id: generateUID(),
                                                icon: MynahIcons.TRASH,
                                                text: 'Delete',
                                            },
                                        ],
                                    },
                                    {
                                        title: 'Another memory',
                                        description: 'Inferred by Q at 2:45pm on 1/2/24',
                                        actions: [
                                            {
                                                id: generateUID(),
                                                icon: MynahIcons.PENCIL,
                                                text: 'Edit',
                                            },
                                            {
                                                id: generateUID(),
                                                icon: MynahIcons.TRASH,
                                                text: 'Delete',
                                            },
                                        ],
                                    },
                                ],
                            },
                            {
                                groupName: '4 days ago',
                                children: [
                                    {
                                        title: 'Another memory',
                                        description: 'Inferred by Q at 2:45pm on 1/2/24',
                                        actions: [
                                            {
                                                id: generateUID(),
                                                icon: MynahIcons.PENCIL,
                                                text: 'Edit',
                                            },
                                            {
                                                id: generateUID(),
                                                icon: MynahIcons.TRASH,
                                                text: 'Delete',
                                            },
                                        ],
                                    },
                                    {
                                        title: 'Another memory',
                                        description: 'Inferred by Q at 2:45pm on 1/2/24',
                                        actions: [
                                            {
                                                id: generateUID(),
                                                icon: MynahIcons.PENCIL,
                                                text: 'Edit',
                                            },
                                            {
                                                id: generateUID(),
                                                icon: MynahIcons.TRASH,
                                                text: 'Delete',
                                            },
                                        ],
                                    },
                                ],
                            },
                            {
                                groupName: 'Last week',
                                children: [
                                    {
                                        title: 'Another memory',
                                        description: 'Inferred by Q at 2:45pm on 1/2/24',
                                        actions: [
                                            {
                                                id: generateUID(),
                                                icon: MynahIcons.PENCIL,
                                                text: 'Edit',
                                            },
                                            {
                                                id: generateUID(),
                                                icon: MynahIcons.TRASH,
                                                text: 'Delete',
                                            },
                                        ],
                                    },
                                    {
                                        title: 'Another memory',
                                        description: 'Inferred by Q at 2:45pm on 1/2/24',
                                        actions: [
                                            {
                                                id: generateUID(),
                                                icon: MynahIcons.PENCIL,
                                                text: 'Edit',
                                            },
                                            {
                                                id: generateUID(),
                                                icon: MynahIcons.TRASH,
                                                text: 'Delete',
                                            },
                                        ],
                                    },
                                    {
                                        title: 'Another memory',
                                        description: 'Inferred by Q at 2:45pm on 1/2/24',
                                        actions: [
                                            {
                                                id: generateUID(),
                                                icon: MynahIcons.PENCIL,
                                                text: 'Edit',
                                            },
                                            {
                                                id: generateUID(),
                                                icon: MynahIcons.TRASH,
                                                text: 'Delete',
                                            },
                                        ],
                                    },
                                    {
                                        title: 'Another memory',
                                        description: 'Inferred by Q at 2:45pm on 1/2/24',
                                        actions: [
                                            {
                                                id: generateUID(),
                                                icon: MynahIcons.PENCIL,
                                                text: 'Edit',
                                            },
                                            {
                                                id: generateUID(),
                                                icon: MynahIcons.TRASH,
                                                text: 'Delete',
                                            },
                                        ],
                                    },
                                    {
                                        title: 'Another memory',
                                        description: 'Inferred by Q at 2:45pm on 1/2/24',
                                        actions: [
                                            {
                                                id: generateUID(),
                                                icon: MynahIcons.PENCIL,
                                                text: 'Edit',
                                            },
                                            {
                                                id: generateUID(),
                                                icon: MynahIcons.TRASH,
                                                text: 'Delete',
                                            },
                                        ],
                                    },
                                ],
                            },
                        ],
                    },
                    events: {
                        onFilterValueChange: (filterValues: Record<string, any>, isValid: boolean) => {
                            Log('Filter changed');
                        },
                        onKeyPress: (e) => {
                            Log('Key pressed');
                            if (e.key === KeyMap.ESCAPE) {
                                close();
                            }
                        },
                        onItemSelect: (detailedListItem) => {
                            Log('Item selected');
                        },
                        onActionClick: (button) => {
                            Log('Action clicked');
                        },
                        onClose: () => {
                            Log('Sheet closed');
                        },
                    },
                });
                // update({

                // });
            } else if (buttonId === 'save-session') {
                localStorage.setItem('mynah-ui-storage', JSON.stringify(mynahUI.getAllTabs()));
            } else if (buttonId === 'remove-saved-session') {
                localStorage.removeItem('mynah-ui-storage');
                window.location.reload();
            } else if (buttonId === 'new-welcome-screen') {
                mynahUI.updateStore('', {
                    ...mynahUIDefaults.store,
                    ...welcomeScreenTabData.store,
                });
            } else if (buttonId === 'account-details') {
                mynahUI.updateStore('', accountDetailsTabData);
            } else if (buttonId === 'export-chat-md') {
                const serializedChat = mynahUI.serializeChat(tabId, 'markdown');
                const blob = new Blob([serializedChat], { type: 'text/plain' });
                const url = URL.createObjectURL(blob);
                const link = document.createElement('a');
                link.download = 'exported-chat.md';
                link.href = url;
                link.click();

                mynahUI.notify({
                    type: NotificationType.SUCCESS,
                    title: 'Chat exported',
                    content: 'The file will be downloaded.',
                });
            } else if (buttonId === 'export-chat-html') {
                const serializedChat = mynahUI.serializeChat(tabId, 'html');
                const blob = new Blob([serializedChat], { type: 'text/plain' });
                const url = URL.createObjectURL(blob);
                const link = document.createElement('a');
                link.download = 'exported-chat.html';
                link.href = url;
                link.click();

                mynahUI.notify({
                    type: NotificationType.SUCCESS,
                    title: 'Chat exported',
                    content: 'The file will be downloaded.',
                });
            } else if (buttonId === 'enable-disable-progress-bar') {
                const currStatus = mynahUI.getTabData(tabId).getStore();
                if (currStatus?.promptInputProgress != null) {
                    mynahUI.updateStore(tabId, { promptInputProgress: null });
                } else {
                    mynahUI.updateStore(tabId, {
                        promptInputProgress: {
                            status: 'default',
                            text: 'Progressing...',
                            value: -1,
                        },
                    });
                }
            } else if (buttonId === 'animation-fast') {
                // Recreate MynahUI with fast animation settings
                window.location.hash = 'fast-animation';
                window.location.reload();
            } else if (buttonId === 'animation-normal') {
                // Recreate MynahUI with normal animation settings
                window.location.hash = 'normal-animation';
                window.location.reload();
            } else if (buttonId === 'animation-disabled') {
                // Recreate MynahUI with disabled animation
                window.location.hash = 'no-animation';
                window.location.reload();
            }
            Log(`Tab bar button clicked when tab ${tabId} is selected: <b>${buttonId}</b>`);
        },
        onTabAdd: (tabId: string) => {
            Log(`New tab added: <b>${tabId}</b>`);
        },
        onSearchShortcut: (tabId: string) => {
            Log(`Search shortcut pressed on tab: <b>${tabId}</b>`);
        },
        onOpenFileDialogClick: (tabId: string, fileType: string, insertPosition: number) => {

            if (fileType === 'image') {
                // Get the current selected tab
                const selectedTab = Object.entries(mynahUI.getAllTabs()).find(([_, tab]) => tab.isSelected)?.[0];
                if (!selectedTab) return false;

                // Create a file input element
                const fileInput = document.createElement('input');
                fileInput.type = 'file';
                fileInput.accept = 'image/*';
                fileInput.style.display = 'none';

                // Add it to the document
                document.body.appendChild(fileInput);

                // Handle file selection
                fileInput.onchange = (e) => {
                    const file = (e.target as HTMLInputElement).files?.[0];
                    if (file) {
                        // Create a context item for the selected file
                        const contextItem: QuickActionCommand = {
                            command: file.name,
                            icon: MynahIcons.IMAGE,
                            label: 'image',
                            route: [file.name],
                            description: '/User/Sample/' + file.name
                        };
                        // Return true to allow the context item to be inserted
                        // The original context item will be replaced with our file context item
                        mynahUI.addCustomContextToPrompt(selectedTab, [contextItem], insertPosition)
                        Log(`Image context added by typing '@image:': <b>${contextItem.command}</b>`);
                        return true;
                    }

                    // Clean up
                    document.body.removeChild(fileInput);
                    return false;
                };

                // Trigger the file dialog
                fileInput.click();
                return false;
            }
        },

        onContextSelected(contextItem, tabId) {
            if (contextItem.command.toLowerCase() === 'image') {
                const fileInput = document.createElement('input');
                fileInput.type = 'file';
                fileInput.accept = 'image/*';
                fileInput.style.display = 'none';

                // Add it to the document
                document.body.appendChild(fileInput);

                // Handle file selection
                fileInput.onchange = (e) => {
                    const file = (e.target as HTMLInputElement).files?.[0];
                    if (file) {
                        // Create a context item for the selected file
                        const contextItem: QuickActionCommand = {
                            command: file.name,
                            icon: MynahIcons.IMAGE,
                            label: 'image',
                            route: [file.name],
                            description: '/User/Sample/' + file.name
                        };

                        mynahUI.addCustomContextToPrompt(tabId, [contextItem])
                        Log(`Image context added by selecting Image from context menu: <b>${contextItem.command}</b>`);
                        // avoid insert of context
                        return false;
                    }

                    // Clean up
                    document.body.removeChild(fileInput);
                    return false;
                };

                // Trigger the file dialog
                fileInput.click();
                return false;
            }
            if (contextItem.command === 'Add Prompt') {
                Log('Custom context action triggered for adding a prompt!');
                return false;
            }
            return true;
        },
        onBeforeTabRemove: (tabId: string): boolean => {
            const isTabLoading = mynahUI.getAllTabs()[tabId].store?.loadingChat;
            if (isTabLoading) {
                Log(`Confirmation Popup appeared on tab remove: <b>${tabId}</b>`);
            }
            return !isTabLoading;
        },
        onTabRemove: (tabId: string) => {
            Log(`Tab removed: <b>${tabId}</b>`);
        },
        onTabChange: (tabId: string) => {
            Log(`Tab changed to: <b>${tabId}</b>`);
        },
        onSendFeedback: (tabId: string, feedbackPayload: FeedbackPayload) => {
            Log(`Feedback sent <br/>
      type: <b>${feedbackPayload.selectedOption}</b><br/>
      comment: <b>${feedbackPayload.comment ?? 'no comment'}</b>`);
            if (feedbackPayload.comment !== undefined) {
                mynahUI.notify({
                    type: NotificationType.INFO,
                    title: 'Your feedback is sent',
                    content: 'Thanks for your feedback.',
                });
            }
        },
        onShowMoreWebResultsClick: (tabId, messageId) => {
            Log(`Show more sources clicked for tab <b>${tabId}/${messageId}</b> in message <b>${messageId}</b>`);
        },
        onCopyCodeToClipboard: (
            tabId,
            messageId,
            code,
            type,
            referenceTrackerInformation,
            eventId,
            codeBlockIndex,
            totalCodeBlocks,
        ) => {
            Log(`Code copied to clipboard from tab <b>${tabId}</b> inside message <b>${messageId}</b><br/>
        type: <b>${type ?? 'unknown'}</b><br/>
        code: <b>${escapeHTML(code ?? '')}</b><br/>
        referenceTracker: <b>${referenceTrackerInformation?.map((rt) => rt.information).join('<br/>') ?? ''}</b><br/>
        codeBlockIndex: <b>${(codeBlockIndex ?? 0) + 1}</b> of ${totalCodeBlocks}
      `);
        },
        onCodeInsertToCursorPosition: (
            tabId,
            messageId,
            code,
            type,
            referenceTrackerInformation,
            eventId,
            codeBlockIndex,
            totalCodeBlocks,
        ) => {
            Log(`Code insert to position clicked on tab <b>${tabId}</b> inside message <b>${messageId}</b><br/>
        type: <b>${type ?? 'unknown'}</b><br/>
        code: <b>${escapeHTML(code ?? '')}</b><br/>
        referenceTracker: <b>${referenceTrackerInformation?.map((rt) => rt.information).join('<br/>') ?? ''}</b><br/>
        codeBlockIndex: <b>${(codeBlockIndex ?? 0) + 1}</b> of ${totalCodeBlocks}
      `);
        },
        onCodeBlockActionClicked: (
            tabId,
            messageId,
            actionId,
            data,
            code,
            type,
            referenceTrackerInformation,
            eventId,
            codeBlockIndex,
            totalCodeBlocks,
        ) => {
            Log(`Code action <b>${actionId}</b> clicked on tab <b>${tabId}</b> inside message <b>${messageId}</b><br/>
        type: <b>${type ?? 'unknown'}</b><br/>
        data: <b>${JSON.stringify(data ?? {})}</b><br/>
        code: <b>${escapeHTML(code ?? '')}</b><br/>
        referenceTracker: <b>${referenceTrackerInformation?.map((rt) => rt.information).join('<br/>') ?? ''}</b><br/>
        codeBlockIndex: <b>${(codeBlockIndex ?? 0) + 1}</b> of ${totalCodeBlocks}
      `);
        },
        onChatPrompt: (tabId: string, prompt: ChatPrompt) => {

            

            Log(`New prompt on tab: <b>${tabId}</b><br/>
      prompt: <b>${prompt.prompt !== undefined && prompt.prompt !== '' ? prompt.prompt : '{command only}'}</b><br/>
      command: <b>${prompt.command ?? '{none}'}</b><br/>
      options: <b>{${
          Object.keys(prompt.options ?? {})
              .map((op) => `'${op}': '${prompt.options?.[op] as string}'`)
              .join(',') ?? ''
      }}</b><br/>
      context: <b>[${(prompt.context ?? []).map((ctx) => `${JSON.stringify(ctx)}`).join(']</b>, <b>[')}]`);
            if (tabId === 'tab-1') {
                mynahUI.updateStore(tabId, {
                    tabCloseConfirmationMessage: `Working on "${prompt.prompt}"`,
                });
            }
            if (mynahUI.getAllTabs()[tabId].store?.compactMode) {
                mynahUI.updateStore(tabId, {
                    compactMode: false,
                    tabHeaderDetails: null,
                    ...mynahUIDefaults.store,
                    chatItems: [],
                    tabBackground: false,
                    promptInputLabel: null,
                });
            }
            onChatPrompt(tabId, prompt);
        },
        onStopChatResponse: (tabId: string) => {
            streamingMessageId = null;
            mynahUI.updateStore(tabId, {
                loadingChat: false,
            });
            mynahUI.addChatItem(tabId, {
                type: ChatItemType.DIRECTIVE,
                contentHorizontalAlignment: 'center',
                body: 'You stopped the chat response!',
            });
            Log(`Stop generating code: <b>${tabId}</b>`);
        },
        onFollowUpClicked: (tabId: string, messageId: string, followUp: ChatItemAction) => {
            Log(`Followup click: <b>${followUp.pillText}</b>`);
            if (followUp.prompt != null || followUp.command != null) {
                if (followUp.command === Commands.REPLACE_FOLLOWUPS) {
                    mynahUI.addChatItem(tabId, {
                        type: ChatItemType.ANSWER,
                        messageId: 'my-message-id',
                        body: 'Hello',
                    });

                    setTimeout(() => {
                        mynahUI.updateChatAnswerWithMessageId(tabId, 'my-message-id', {
                            followUp: exampleRichFollowups.followUp,
                        });
                        setTimeout(() => {
                            mynahUI.updateChatAnswerWithMessageId(tabId, 'my-message-id', {
                                followUp: defaultFollowUps.followUp,
                            });
                        }, 1500);
                    }, 1500);
                } else {
                    if (followUp.command != null) {
                        mynahUI.addChatItem(tabId, {
                            type: ChatItemType.PROMPT,
                            body: `Example: **${followUp.pillText}**
              <sub><sup>_can be triggered with **${followUp.command}**_</sup></sub>`,
                        });
                    }
                    onChatPrompt(tabId, {
                        command: followUp.command,
                        prompt: followUp.prompt,
                        escapedPrompt: followUp.escapedPrompt ?? followUp.prompt,
                    });
                }
            }
        },
        onChatPromptProgressActionButtonClicked: (tabId: string, action) => {
            Log(`Chat prompt progress action clicked on tab <b>${tabId}</b>:<br/>
      Action Id: <b>${action.id}</b><br/>
      Action Text: <b>${action.text}</b><br/>
      `);

            if (action.id === 'cancel-running-task') {
                streamingMessageId = null;
                mynahUI.updateStore(tabId, {
                    loadingChat: false,
                });
                Log(`Stop generating code: <b>${tabId}</b>`);
            }
        },
        onTabbedContentTabChange: (tabId: string, messageId: string, contentTabId: string) => {
            Log(`Tabbed content tab changed on tab <b>${tabId}</b>:<br/>
        Message Id: <b>${messageId}</b><br/>
        Content tabId: <b>${contentTabId}</b><br/>
        `);
        },
        onFilesDropped: async (tabId: string, files: FileList, insertPosition: number) => {
            const allowedFileTypes = ["image/png", "image/jpeg", "image/gif", "image/webp"];
            const commands: QuickActionCommand[] = [];
            for (const file of Array.from(files)) {
                if (allowedFileTypes.includes(file.type)) {
                    const arrayBuffer = await file.arrayBuffer();
                    const bytes = new Uint8Array(arrayBuffer);
                    const contextItem: CustomQuickActionCommand = {
                        command: file.name,
                        icon: MynahIcons.IMAGE,
                        label: 'image',
                        route: [file.name],
                        description: '/User/Sample/' + file.name,
                        content: bytes
                    };
                    commands.push(contextItem);
                }
            }
            mynahUI.addCustomContextToPrompt(tabId, commands, insertPosition);
            Log(`Images dropped: ${commands.map(cmd => `<br/>- <b>${cmd.command}</b>`).join('')}`);
        },
        onInBodyButtonClicked: (tabId: string, messageId: string, action) => {
            if (action.id === 'allow-readonly-tools') {
                mynahUI.updateChatAnswerWithMessageId(tabId, messageId, {
                    muted: true,
                    header: {
                        icon: 'tools',
                        body: '#### Allow read-only tools outside your workspace',
                        buttons: [
                            {
                                id: 'allow-readonly-tools',
                                text: 'Allowed',
                                icon: 'ok',
                                status: 'clear',
                                disabled: true,
                            },
                        ],
                    },
                });
            } else if (action.id === 'accept-file-change-on-header-card') {
                mynahUI.updateChatAnswerWithMessageId(tabId, messageId, {
                    muted: true,
                    header: {
                        ...(mynahUI
                            .getTabData(tabId)
                            ?.getStore()
                            ?.chatItems?.find((chatItem: any) => chatItem.messageId === messageId)?.header ?? {}),
                        buttons: null,
                        status: {
                            icon: 'ok',
                            status: 'success',
                            text: 'Accepted',
                        },
                    },
                });
            } else if (action.id === 'reject-file-change-on-header-card') {
                mynahUI.updateChatAnswerWithMessageId(tabId, messageId, {
                    muted: true,
                    header: {
                        ...(mynahUI
                            .getTabData(tabId)
                            ?.getStore()
                            ?.chatItems?.find((chatItem: any) => chatItem.messageId === messageId)?.header ?? {}),
                        buttons: null,
                        status: {
                            icon: 'cancel',
                            status: 'error',
                            text: 'Rejected',
                        },
                    },
                });
            } else if (action.id === 'quick-start') {
                mynahUI.updateStore(tabId, {
                    tabHeaderDetails: null,
                    compactMode: false,
                    tabBackground: false,
                    promptInputText: '/dev',
                    promptInputLabel: null,
                    chatItems: [],
                });
            }
            if (action.id === 'explore') {
                mynahUI.updateStore('', exploreTabData);
            }
            if (action.id.match('quick-start-')) {
                mynahUI.updateStore('', {
                    ...mynahUIDefaults.store,
                    chatItems: [],
                    promptInputText: `/${action.id.replace('quick-start-', '')}`,
                    quickActionCommands: qAgentQuickActions,
                });
            }
            if (messageId === 'sticky-card') {
                mynahUI.updateStore(tabId, { promptInputStickyCard: null });
            }
            Log(`Body action clicked in message <b>${messageId}</b>:<br/>
      Action Id: <b>${action.id}</b><br/>
      Action Text: <b>${action.text}</b><br/>
      ${
          action.formItemValues
              ? `<br/>Options:<br/>${Object.keys(action.formItemValues)
                    .map((optionId) => {
                        return `<b>${optionId}</b>: ${(action.formItemValues as Record<string, string>)[optionId] ?? ''}`;
                    })
                    .join('<br/>')}`
              : ''
      }
      `);
        },
        onQuickCommandGroupActionClick: (tabId: string, action) => {
            Log(`Quick command group action clicked in tab <b>${tabId}</b>:<br/>
      Action Id: <b>${action.id}</b><br/>
      `);
        },
        onVote: (tabId: string, messageId: string, vote: RelevancyVoteType) => {
            Log(`Message <b>${messageId}</b> is <b>${vote}d</b>.`);
        },
        onFileClick: (
            tabId: string,
            filePath: string,
            deleted: boolean,
            messageId?: string,
            eventId?: string,
            fileDetails?: TreeNodeDetails,
        ) => {
            Log(`File clicked on message ${messageId}: <b>${filePath}</b>`);
        },
        onFileActionClick: (tabId, messageId, filePath, actionName) => {
            Log(`File action clicked on message ${messageId}: <b>${filePath}</b> -> ${actionName}`);
            switch (actionName) {
                case 'reject-change':
                    mynahUI.updateChatAnswerWithMessageId(tabId, messageId, exampleFileListChatItemForUpdate);
                    break;
                case 'show-diff':
                    mynahUI.updateChatAnswerWithMessageId(tabId, messageId, {
                        body: exampleCodeDiff,
                    });
                    break;
                case 'revert-rejection':
                    mynahUI.updateChatAnswerWithMessageId(tabId, messageId, {
                        fileList: exampleFileListChatItem.fileList,
                    });
                    break;
                default:
                    break;
            }
        },
        onFormModifierEnterPress(formData, tabId) {
            Log(`Form modifier enter pressed on tab <b>${tabId}</b>:<br/>
      Form data: <b>${JSON.stringify(formData)}</b><br/>
      `);
        },
        onFormTextualItemKeyPress(event, formData, itemId, tabId) {
            Log(`Form keypress on tab <b>${tabId}</b>:<br/>
      Item id: <b>${itemId}</b><br/>
      Key: <b>${event.keyCode}</b><br/>
      `);
            if (
                itemId === 'prompt-name' &&
                event.key === 'Enter' &&
                event.ctrlKey !== true &&
                event.shiftKey !== true
            ) {
                event.preventDefault();
                event.stopImmediatePropagation();
                Log(`Form keypress Enter submit on tab <b>${tabId}</b>:<br/>
          ${
              formData
                  ? `<br/>Options:<br/>${Object.keys(formData)
                        .map((optionId) => {
                            return `<b>${optionId}</b>: ${(formData as Record<string, string>)[optionId] ?? ''}`;
                        })
                        .join('<br/>')}`
                  : ''
          }
          `);
                return true;
            }
            return false;
        },
        onCustomFormAction: (tabId, action) => {
            Log(`Custom form action clicked for tab <b>${tabId}</b>:<br/>
      Action Id: <b>${action.id}</b><br/>
      Action Text: <b>${action.text}</b><br/>
      ${
          action.formItemValues
              ? `<br/>Options:<br/>${Object.keys(action.formItemValues)
                    .map((optionId) => {
                        return `<b>${optionId}</b>: ${(action.formItemValues as Record<string, string>)[optionId] ?? ''}`;
                    })
                    .join('<br/>')}`
              : ''
      }
      `);
        },
        onChatItemEngagement: (tabId, messageId, engagement) => {
            Log(`<b>${engagement.engagementType}</b> in message <b>${messageId}</b><br/>
      Engagement duration: <b>${engagement.engagementDurationTillTrigger}</b>ms <br/>
      Total X distance: <b>${engagement.totalMouseDistanceTraveled.x}</b>px <br/>
      Total Y distance: <b>${engagement.totalMouseDistanceTraveled.y}</b>px <br/>
      Selection X distance: <b>${engagement.selectionDistanceTraveled?.x ?? '0'}px</b> <br/>
      Selection Y distance: <b>${engagement.selectionDistanceTraveled?.y ?? '0'}px</b>`);
        },
        onLinkClick: (tabId, messageId, link, mouseEvent) => {
            if (link === '#open-diff-viewer') {
                mouseEvent?.preventDefault();
                Log(`Open diff viewer clicked`);
            }
            Log(`Link inside body clicked: <b>${link}</b>`);
        },
        onFormLinkClick: (link, mouseEvent) => {
            Log(`Link inside form clicked: <b>${link}</b>`);
        },
        onSourceLinkClick: (tabId, messageId, link, mouseEvent) => {
            Log(`Link in sources clicked: <b>${link}</b>`);
        },
        onInfoLinkClick: (tabId, link, mouseEvent) => {
            Log(`Link inside prompt info field clicked: <b>${link}</b>`);
        },
        onMessageDismiss: (tabId, messageId) => {
            Log(`Card dismissed: tabId: <b>${tabId}</b>, messageId: <b>${messageId}</b>`);
        },
    });

    setTimeout(() => {
        mynahUI.toggleSplashLoader(false);
    }, INITIAL_STREAM_DELAY);

    const onChatPrompt = (tabId: string, prompt: ChatPrompt) => {
        if (prompt.command !== undefined && prompt.command.trim() !== '') {
            switch (prompt.command) {
                case Commands.INSERT_CODE:
                    mynahUI.addToUserPrompt(tabId, exampleCodeBlockToInsert, 'code');
                    break;
                case Commands.CLEAR:
                    mynahUI.updateStore(tabId, {
                        chatItems: [],
                    });
                    break;
                case Commands.CLEAR_CONTEXT_ITEMS:
                    mynahUI.updateStore(tabId, {
                        contextCommands: [],
                    });
                    break;
                case Commands.CLEAR_LOGS:
                    LogClear();
                    break;
                case Commands.NOTIFY:
                    mynahUI.notify({
                        content: 'Click this notification to remove it. It does not have a duration.',
                        duration: -1,
                        type: NotificationType.INFO,
                        title: 'Notification!!',
                        onNotificationClick: () => {
                            Log('Sample notification clicked.');
                        },
                        onNotificationHide: () => {
                            Log('Sample notification removed.');
                        },
                    });
                    break;
                case Commands.FORM_CARD:
                    mynahUI.addChatItem(tabId, exampleFormChatItem);
                    mynahUI.addChatItem(tabId, defaultFollowUps);
                    break;
                case Commands.VOTE:
                    mynahUI.addChatItem(tabId, exampleVoteChatItem);
                    mynahUI.addChatItem(tabId, defaultFollowUps);
                    break;
                case Commands.CARD_WITH_MARKDOWN_LIST:
                    getGenerativeAIAnswer(tabId, sampleMarkdownList);
                    break;
                case Commands.CARD_WITH_PROGRESSIVE_FILE_LIST:
                    getGenerativeAIAnswer(tabId, sampleProgressiveFileList, ChatItemType.ANSWER);
                    break;
                case Commands.CARD_WITH_ALL_MARKDOWN_TAGS:
                    mynahUI.addChatItem(tabId, {
                        type: ChatItemType.ANSWER,
                        messageId: generateUID(),
                        body: sampleAllInOneList.slice(-1)[0].body,
                        snapToTop: true,
                    });
                    mynahUI.addChatItem(tabId, defaultFollowUps);
                    break;
                case Commands.CARD_RENDER_MARKDOWN_TABLE:
                    mynahUI.addChatItem(tabId, {
                        type: ChatItemType.ANSWER,
                        messageId: generateUID(),
                        body: sampleTableList.slice(-1)[0].body,
                        snapToTop: true,
                    });
                    mynahUI.addChatItem(tabId, defaultFollowUps);
                    break;
                case Commands.CARD_SNAPS_TO_TOP:
                    mynahUI.addChatItem(tabId, {
                        type: ChatItemType.ANSWER,
                        messageId: generateUID(),
                        body: sampleMarkdownList.slice(-1)[0].body,
                        snapToTop: true,
                    });
                    mynahUI.addChatItem(tabId, defaultFollowUps);
                    break;
                case Commands.PROGRESSIVE_CARD:
                    getGenerativeAIAnswer(tabId, exampleProgressCards);
                    break;
                case Commands.HEADER_TYPES:
                    sampleHeaderTypes.forEach((ci) => mynahUI.addChatItem(tabId, ci));
                    break;
                case Commands.SUMMARY_CARD:
                    const cardId = generateUID();
                    mynahUI.addChatItem(tabId, {
                        ...mcpToolRunSampleCardInit,
                        messageId: cardId,
                    });
                    setTimeout(() => {
                        mynahUI.updateChatAnswerWithMessageId(tabId, cardId, mcpToolRunSampleCard);
                        mynahUI.addChatItem(tabId, defaultFollowUps);
                    }, 3000);
                    break;
                case Commands.STATUS_CARDS:
                    mynahUI.addChatItem(tabId, {
                        type: ChatItemType.ANSWER,
                        messageId: new Date().getTime().toString(),
                        muted: true,
                        body: `This is an extended card with an icon and a different border color. It also includes some action buttons.`,
                        status: 'error',
                        icon: MynahIcons.ERROR,
                        buttons: [
                            {
                                text: 'I Understand',
                                id: 'understood',
                                status: 'error',
                                icon: MynahIcons.OK,
                            },
                        ],
                    });
                    mynahUI.addChatItem(tabId, {
                        type: ChatItemType.ANSWER,
                        messageId: new Date().getTime().toString(),
                        body: `This is an extended card with an icon and a different border color. Including some action buttons.`,
                        status: 'info',
                        icon: MynahIcons.INFO,
                        buttons: [
                            {
                                text: 'Acknowledge',
                                id: 'ack',
                                status: 'info',
                                icon: MynahIcons.OK,
                            },
                        ],
                    });
                    mynahUI.addChatItem(tabId, {
                        type: ChatItemType.ANSWER,
                        messageId: new Date().getTime().toString(),
                        body: `This is an extended card with an icon and a different border color. Including some action buttons.`,
                        status: 'warning',
                        icon: MynahIcons.WARNING,
                    });
                    mynahUI.addChatItem(tabId, {
                        type: ChatItemType.ANSWER,
                        messageId: new Date().getTime().toString(),
                        body: `You're doing very good. Awesome work mate!`,
                        status: 'success',
                        icon: MynahIcons.THUMBS_UP,
                        buttons: [
                            {
                                text: 'Yay!',
                                id: 'yay',
                                status: 'success',
                            },
                        ],
                    });
                    mynahUI.addChatItem(tabId, defaultFollowUps);
                    break;
                case Commands.SHOW_STICKY_CARD:
                    mynahUI.updateStore(tabId, {
                        promptInputStickyCard: {
                            messageId: 'sticky-card',
                            canBeDismissed: true,
                            header: {
                                icon: 'code-block',
                                iconStatus: 'info',
                                body: '### Terms and Conditions',
                            },
                            body: `Our [Terms and Conditions](#) are updated. Please review and read it. To accept please hit the **Acknowledge** button.`,
                            buttons: [
                                {
                                    text: 'Acknowledge',
                                    id: 'acknowledge',
                                    status: 'info',
                                },
                            ],
                        },
                    });
                    break;
                case Commands.FILE_LIST_CARD:
                    mynahUI.addChatItem(tabId, {
                        ...exampleFileListChatItem,
                        messageId: `FILE_LIST_${new Date().getTime().toString()}`,
                    });
                    mynahUI.addChatItem(tabId, defaultFollowUps);
                    break;
                case Commands.FOLLOWUPS_AT_RIGHT:
                    mynahUI.addChatItem(tabId, exampleRichFollowups);
                    break;
                case Commands.INFORMATION_CARDS:
                    mynahUI.addChatItem(tabId, exampleInformationCard(null, null, true));
                    mynahUI.addChatItem(
                        tabId,
                        exampleInformationCard(
                            'warning',
                            'You have hit the usage limit for this chat bot. Contact your admin to enable usage overages or learn more about pro license limits.',
                        ),
                    );
                    mynahUI.addChatItem(
                        tabId,
                        exampleInformationCard(
                            'error',
                            'You have hit the usage limit for this chat bot. Contact your admin to enable usage overages or learn more about pro license limits.',
                        ),
                    );
                    mynahUI.addChatItem(tabId, exampleInformationCard('success', 'Successfully completed this task!'));
                    mynahUI.addChatItem(tabId, defaultFollowUps);
                    break;
                case Commands.BORDERED_CARDS:
                    mynahUI.addChatItem(tabId, exampleBorderedCard());
                    mynahUI.addChatItem(tabId, defaultFollowUps);
                    break;
                case Commands.CONFIRMATION_BUTTONS:
                    mynahUI.addChatItem(tabId, exampleConfirmationButtons);
                    mynahUI.addChatItem(tabId, defaultFollowUps);
                    break;
                case Commands.BUTTONS:
                    mynahUI.addChatItem(tabId, exampleButtons);
                    mynahUI.addChatItem(tabId, exampleStatusButtons);
                    mynahUI.addChatItem(tabId, defaultFollowUps);
                    break;
                case Commands.SHOW_CUSTOM_FORM:
                    showCustomForm(tabId);
                    break;
                case Commands.IMAGE_IN_CARD:
                    mynahUI.addChatItem(tabId, exampleImageCard());
                    mynahUI.addChatItem(tabId, defaultFollowUps);
                    break;
                case Commands.CUSTOM_RENDERER_CARDS:
                    mynahUI.addChatItem(tabId, exampleCustomRendererWithHTMLMarkup());
                    mynahUI.addChatItem(tabId, exampleCustomRendererWithDomBuilderJson);
                    mynahUI.addChatItem(tabId, defaultFollowUps);
                    break;
                case Commands.COMMAND_WITH_PROMPT:
                    const realPromptText = prompt.escapedPrompt?.trim() ?? '';
                    mynahUI.addChatItem(tabId, {
                        type: ChatItemType.PROMPT,
                        messageId: new Date().getTime().toString(),
                        body: `${Commands.COMMAND_WITH_PROMPT} => ${realPromptText}`,
                    });
                    getGenerativeAIAnswer(tabId);
                    break;
                case '/animation-demo':
                    mynahUI.addChatItem(tabId, {
                        type: ChatItemType.PROMPT,
                        messageId: new Date().getTime().toString(),
                        body: 'Test streaming animation speeds',
                    });
                    // Create a longer streaming response to better demonstrate animation differences
                    const longStreamParts = [
                        { body: 'Here is a demonstration of the typewriter animation speed. ' },
                        { body: 'This response will stream word by word to show the animation effect. ' },
                        { body: 'You can use the tab bar buttons (Fast Animation, Normal Animation, Disable Animation) to switch between different modes. ' },
                        { body: 'Fast animation uses 100ms stack time with 20ms max per word. ' },
                        { body: 'Normal animation uses 500ms stack time with 50ms max per word. ' },
                        { body: 'Disabled animation shows content immediately without any delay. ' },
                        { body: 'Try switching modes and running this command again to see the difference! ' },
                        { body: '\n\n```typescript\n// Animation configuration\nconst config = {\n  typewriterStackTime: 100, // Fast mode\n  typewriterMaxWordTime: 20,\n  // OR\n  disableTypewriterAnimation: true // Instant\n};\n```' },
                    ];
                    getGenerativeAIAnswer(tabId, longStreamParts);
                    break;
                default:
                    mynahUI.addChatItem(tabId, {
                        type: ChatItemType.PROMPT,
                        messageId: new Date().getTime().toString(),
                        body: `**${prompt.command.replace('/', '')}**\n${prompt.escapedPrompt as string}`,
                    });
                    getGenerativeAIAnswer(tabId);
                    break;
            }
        } else {
            if (prompt != null) {
                mynahUI.addChatItem(tabId, {
                    type: ChatItemType.PROMPT,
                    autoCollapse: true,
                    messageId: new Date().getTime().toString(),
                    body: `${prompt.escapedPrompt as string}`,
                });
            }
            getGenerativeAIAnswer(tabId);
        }
    };

    const showCustomForm = (tabId: string) => {
        mynahUI.showCustomForm(
            tabId,
            [
                {
                    type: 'textinput',
                    id: 'prompt-name',
                    title: 'Prompt name',
                    mandatory: true,
                    validationPatterns: {
                        patterns: [{ pattern: /^[^./\\]+$/ }],
                        genericValidationErrorMessage:
                            'Text cannot contain dots (.), forward slashes (/), or backslashes (\\).',
                    },
                    placeholder: 'Enter prompt name',
                    description: "Use this prompt by typing '@' followed by the prompt name.",
                    autoFocus: true,
                    validateOnChange: true
                },
            ],
            [
                {
                    id: 'cancel-create-prompt',
                    status: 'clear',
                    text: 'Cancel',
                    waitMandatoryFormItems: false,
                },
                {
                    id: 'submit-create-prompt',
                    text: 'Create',
                    status: 'main',
                    waitMandatoryFormItems: true,
                },
            ],
            'Create saved prompt',
        );
    };

    const getGenerativeAIAnswer = (tabId: string, optionalParts?: Partial<ChatItem>[], type?: ChatItemType): void => {
        const messageId = generateUID();
        mynahUI.updateStore(tabId, {
            loadingChat: true,
            promptInputDisabledState: true,
        });
        mynahUI.addChatItem(tabId, {
            type: type ?? ChatItemType.ANSWER_STREAM,
            body: type === ChatItemType.ANSWER && optionalParts?.[0]?.body != null ? optionalParts[0].body : '',
            shimmer: type === ChatItemType.ANSWER && optionalParts?.[0]?.body != null,
            messageId: messageId,
        });
        connector
            .requestGenerativeAIAnswer(
                optionalParts ?? [
                    {
                        ...exampleStreamParts[0],
                        // messageId,
                        header: {
                            fileList: {
                                collapsed: true,
                                hideFileCount: true,
                                flatList: true,
                                rootFolderTitle: 'Context',
                                folderIcon: null,
                                fileTreeTitle: '',
                                filePaths: ['./src/index.ts', './main', 'js_expert'],
                                details: {
                                    './src/index.ts': {
                                        icon: MynahIcons.FILE,
                                        description: `**index.ts** under **src** folder is
used as a context to generate this message.`,
                                    },
                                    './main': {
                                        icon: MynahIcons.FOLDER,
                                    },
                                    js_expert: {
                                        icon: MynahIcons.CHAT,
                                    },
                                },
                            },
                        },
                    },
                    {
                        header: undefined,
                    },
                    ...exampleStreamParts,
                ],
                (chatItem: Partial<ChatItem>, percentage: number) => {
                    if (streamingMessageId != null) {
                        mynahUI.updateChatAnswerWithMessageId(tabId, messageId, {
                            ...chatItem,
                            messageId: streamingMessageId,
                        });

                        mynahUI.updateStore(tabId, {
                            ...(optionalParts != null
                                ? {
                                      promptInputProgress: {
                                          status: 'info',
                                          ...(percentage > 50 ? { text: 'Almost done...' } : {}),
                                          valueText: `${parseInt(percentage.toString())}%`,
                                          value: percentage,
                                      },
                                  }
                                : {}),
                        });
                        return false;
                    }
                    return true;
                },
                () => {
                    const cardDetails = mynahUI.endMessageStream(tabId, messageId, {}) as Record<string, any>;
                    mynahUI.endMessageStream(tabId, `${messageId}_clone`, {}) as Record<string, any>;

                    mynahUI.updateStore(tabId, {
                        loadingChat: false,
                        promptInputDisabledState: false,
                    });
                    if (optionalParts != null) {
                        mynahUI.updateStore(tabId, {
                            promptInputProgress: {
                                status: 'success',
                                text: 'Completed...',
                                valueText: '',
                                value: 100,
                                actions: [],
                            },
                        });
                        setTimeout(() => {
                            mynahUI.updateStore(tabId, {
                                promptInputProgress: null,
                            });
                        }, 1500);
                    }
                    Log(`Stream ended with details: <br/>
          ${Object.keys(cardDetails)
              .map((key) => `${key}: <b>${cardDetails[key].toString()}</b>`)
              .join('<br/>')}
          `);
                    mynahUI.addChatItem(tabId, { ...defaultFollowUps, messageId: generateUID() });
                    streamingMessageId = null;
                },
            )
            .then(() => {
                streamingMessageId = messageId;
                mynahUI.updateChatAnswerWithMessageId(tabId, streamingMessageId, {
                    type: ChatItemType.ANSWER_STREAM,
                    body: '',
                    canBeVoted: true,
                });
                if (optionalParts != null) {
                    mynahUI.updateStore(tabId, {
                        promptInputProgress: {
                            status: 'default',
                            text: 'Work in progress...',
                            value: -1,
                            actions: [
                                {
                                    id: 'cancel-running-task',
                                    text: 'Cancel',
                                    icon: MynahIcons.CANCEL,
                                    status: 'clear',
                                    disabled: false,
                                },
                            ],
                        },
                    });
                }
            });
    };

    new ThemeBuilder('#theme-editor');

    return mynahUI;
};

window.mynahUI = createMynahUI();
