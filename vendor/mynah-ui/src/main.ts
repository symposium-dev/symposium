/*!
 * Copyright 2022 Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

import { Notification } from './components/notification';
import { DomBuilder, ExtendedHTMLElement } from './helper/dom';
import {
  MynahPortalNames,
  RelevancyVoteType,
  FeedbackPayload,
  MynahUIDataModel,
  MynahEventNames,
  NotificationType,
  ChatItem,
  ChatItemAction,
  ChatPrompt,
  MynahUITabStoreModel,
  MynahUITabStoreTab,
  ConfigModel,
  ReferenceTrackerInformation,
  CodeSelectionType,
  Engagement,
  ChatItemFormItem,
  ChatItemButton,
  ChatItemType,
  CardRenderDetails,
  PromptAttachmentType,
  QuickActionCommand,
  DetailedList,
  TreeNodeDetails,
  Action,
  DropdownListOption,
} from './static';
import { cancelEvent, MynahUIGlobalEvents } from './helper/events';
import { Tabs } from './components/navigation-tabs';
import { ChatWrapper } from './components/chat-item/chat-wrapper';
import { FeedbackForm } from './components/feedback-form/feedback-form';
import { MynahUITabsStore } from './helper/tabs-store';
import { Config } from './helper/config';
import { generateUID } from './helper/guid';
import { NoTabs } from './components/no-tabs';
import { copyToClipboard } from './helper/chat-item';
import { serializeHtml, serializeMarkdown } from './helper/serialize-chat';
import { Sheet } from './components/sheet';
import { DetailedListSheet, DetailedListSheetProps } from './components/detailed-list/detailed-list-sheet';
import { configureMarked, parseMarkdown } from './helper/marked';
import { MynahUIDataStore } from './helper/store';
import { StyleLoader } from './helper/style-loader';
import { Icon } from './components/icon';
import { Button } from './components/button';
import { TopBarButtonOverlayProps } from './components/chat-item/prompt-input/prompt-top-bar/top-bar-button';

export { generateUID } from './helper/guid';
export {
  ChatItemBodyRenderer,
} from './helper/dom';
export {
  AllowedAttributesInCustomRenderer,
  AllowedTagsInCustomRenderer
} from './helper/sanitize';
export * from './static';
export {
  ToggleOption
} from './components/tabs';
export {
  MynahIcons,
  MynahIconsType
} from './components/icon';
export {
  DomBuilder,
  DomBuilderObject,
  ExtendedHTMLElement,
} from './helper/dom';
export {
  ButtonProps,
  ButtonAbstract
} from './components/button';
export {
  RadioGroupProps,
  RadioGroupAbstract
} from './components/form-items/radio-group';
export {
  SelectProps,
  SelectAbstract
} from './components/form-items/select';
export {
  TextInputProps,
  TextInputAbstract
} from './components/form-items/text-input';
export {
  TextAreaProps,
  TextAreaAbstract
} from './components/form-items/text-area';
export {
  ChatItemCardContent,
  ChatItemCardContentProps
} from './components/chat-item/chat-item-card-content';
export { default as MynahUITestIds } from './helper/test-ids';

export interface MynahUIProps {
  rootSelector?: string;
  loadStyles?: boolean;
  defaults?: MynahUITabStoreTab;
  splashScreenInitialStatus?: {
    visible: boolean;
    text?: string;
    actions?: Action[];
  };
  tabs?: MynahUITabStoreModel;
  config?: Partial<ConfigModel>;
  onShowMoreWebResultsClick?: (
    tabId: string,
    messageId: string,
    eventId?: string) => void;
  onReady?: () => void;
  onFocusStateChanged?: (focusState: boolean) => void;
  onVote?: (
    tabId: string,
    messageId: string,
    vote: RelevancyVoteType,
    eventId?: string) => void;
  onStopChatResponse?: (
    tabId: string,
    eventId?: string) => void;
  onResetStore?: (tabId: string) => void;
  onChatPrompt?: (
    tabId: string,
    prompt: ChatPrompt,
    eventId?: string) => void;
  onChatPromptProgressActionButtonClicked?: (
    tabId: string,
    action: {
      id: string;
      text?: string;
    },
    eventId?: string) => void;
  onFollowUpClicked?: (
    tabId: string,
    messageId: string,
    followUp: ChatItemAction,
    eventId?: string) => void;
  onInBodyButtonClicked?: (
    tabId: string,
    messageId: string,
    action: {
      id: string;
      text?: string;
      formItemValues?: Record<string, string>;
    },
    eventId?: string) => void;
  onTabbedContentTabChange?: (
    tabId: string,
    messageId: string,
    contentTabId: string,
    eventId?: string) => void;
  onTabChange?: (
    tabId: string,
    eventId?: string) => void;
  onTabAdd?: (
    tabId: string,
    eventId?: string) => void;
  onContextSelected?: (
    contextItem: QuickActionCommand,
    tabId: string,
    eventId?: string
  ) => boolean;
  onTabRemove?: (
    tabId: string,
    eventId?: string) => void;
  onSearchShortcut?: (
    tabId: string,
    eventId?: string) => void;
  /**
   * @param tabId tabId which the close button triggered
   * @returns boolean -> If you want to close the tab immediately send true
   */
  onBeforeTabRemove?: (
    tabId: string,
    eventId?: string) => boolean;
  onChatItemEngagement?: (
    tabId: string,
    messageId: string,
    engagement: Engagement) => void;
  onCodeBlockActionClicked?: (
    tabId: string,
    messageId: string,
    actionId: string,
    data?: string,
    code?: string,
    type?: CodeSelectionType,
    referenceTrackerInformation?: ReferenceTrackerInformation[],
    eventId?: string,
    codeBlockIndex?: number,
    totalCodeBlocks?: number,) => void;
  /**
   * @deprecated since version 4.14.0. It will be only used for keyboard, context menu copy actions, not for button actions after version 5.x.x. Use {@link onCodeBlockActionClicked} instead
   */
  onCopyCodeToClipboard?: (
    tabId: string,
    messageId: string,
    code?: string,
    type?: CodeSelectionType,
    referenceTrackerInformation?: ReferenceTrackerInformation[],
    eventId?: string,
    codeBlockIndex?: number,
    totalCodeBlocks?: number,
    data?: any) => void;
  /**
   * @deprecated since version 4.14.0. Will be dropped after version 5.x.x. Use {@link onCodeBlockActionClicked} instead
   */
  onCodeInsertToCursorPosition?: (
    tabId: string,
    messageId: string,
    code?: string,
    type?: CodeSelectionType,
    referenceTrackerInformation?: ReferenceTrackerInformation[],
    eventId?: string,
    codeBlockIndex?: number,
    totalCodeBlocks?: number,
    data?: any) => void;
  onSourceLinkClick?: (
    tabId: string,
    messageId: string,
    link: string,
    mouseEvent?: MouseEvent,
    eventId?: string) => void;
  onLinkClick?: (
    tabId: string,
    messageId: string,
    link: string,
    mouseEvent?: MouseEvent,
    eventId?: string) => void;
  onInfoLinkClick?: (
    tabId: string,
    link: string,
    mouseEvent?: MouseEvent,
    eventId?: string) => void;
  onFormLinkClick?: (
    link: string,
    mouseEvent?: MouseEvent,
    eventId?: string) => void;
  onSendFeedback?: (
    tabId: string,
    feedbackPayload: FeedbackPayload,
    eventId?: string) => void;
  onFormModifierEnterPress?: (
    formData: Record<string, string>,
    tabId: string,
    eventId?: string) => void;
  onFormTextualItemKeyPress?: (
    event: KeyboardEvent,
    formData: Record<string, string>,
    itemId: string,
    tabId: string,
    eventId?: string) => boolean;
  onFormChange?: (
    formData: Record<string, any>,
    isValid: boolean,
    tabId: string) => void;
  onCustomFormAction?: (
    tabId: string,
    action: {
      id: string;
      text?: string;
      formItemValues?: Record<string, string>;
    },
    eventId?: string) => void;
  onDropDownOptionChange?: (
    tabId: string,
    messageId: string,
    value: DropdownListOption[]) => void;
  onDropDownLinkClick?: (
    tabId: string,
    actionId: string,
    destination: string) => void;
  onPromptInputOptionChange?: (
    tabId: string,
    optionsValues: Record<string, string>,
    eventId?: string) => void;
  onPromptInputButtonClick?: (
    tabId: string,
    buttonId: string,
    eventId?: string) => void;
  onPromptTopBarItemAdded?: (tabId: string,
    item: QuickActionCommand, eventId?: string) => void;
  onPromptTopBarItemRemoved?: (tabId: string,
    item: QuickActionCommand, eventId?: string) => void;
  onPromptTopBarButtonClick?: (tabId: string, button: ChatItemButton, eventId?: string) => void;
  /**
   * @deprecated since version 4.6.3. Will be dropped after version 5.x.x. Use {@link onFileClick} instead
   */
  onOpenDiff?: (
    tabId: string,
    filePath: string,
    deleted: boolean,
    messageId?: string,
    eventId?: string) => void;
  onFileClick?: (
    tabId: string,
    filePath: string,
    deleted: boolean,
    messageId?: string,
    eventId?: string,
    fileDetails?: TreeNodeDetails
  ) => void;
  onMessageDismiss?: (
    tabId: string,
    messageId: string,
    eventId?: string) => void;
  onFileActionClick?: (
    tabId: string,
    messageId: string,
    filePath: string,
    actionName: string,
    eventId?: string) => void;
  onTabBarButtonClick?: (
    tabId: string,
    buttonId: string,
    eventId?: string) => void;
  onQuickCommandGroupActionClick?: (
    tabId: string,
    action: {
      id: string;
    },
    eventId?: string) => void;
  onSplashLoaderActionClick?: (
    action: Action,
    eventId?: string) => void;
  onOpenFileDialogClick?: (
    tabId: string,
    fileType: string,
    insertPosition: number
  ) => void;
  onFilesDropped?: (
    tabId: string,
    files: FileList,
    insertPosition: number
  ) => void;
}

export class MynahUI {
  private readonly render: ExtendedHTMLElement;
  private lastEventId: string = '';
  private readonly props: MynahUIProps;
  private readonly splashLoader: ExtendedHTMLElement;
  private readonly splashLoaderText: ExtendedHTMLElement;
  private readonly splashLoaderActions: ExtendedHTMLElement;
  private readonly tabsWrapper: ExtendedHTMLElement;
  private readonly tabContentsWrapper: ExtendedHTMLElement;
  private readonly feedbackForm?: FeedbackForm;
  private readonly sheet?: Sheet;
  private readonly chatWrappers: Record<string, ChatWrapper> = {};

  constructor (props: MynahUIProps) {
    StyleLoader.getInstance(props.loadStyles !== false).load('styles.scss');
    configureMarked();

    this.props = props;
    Config.getInstance(props.config);
    DomBuilder.getInstance(props.rootSelector);
    MynahUITabsStore.getInstance(this.props.tabs, this.props.defaults);
    MynahUIGlobalEvents.getInstance();

    this.splashLoaderText = DomBuilder.getInstance().build({
      type: 'div',
      classNames: [ 'mynah-ui-splash-loader-text' ],
      innerHTML: parseMarkdown(this.props.splashScreenInitialStatus?.text ?? '', { includeLineBreaks: true }),
    });

    this.splashLoaderActions = DomBuilder.getInstance().build({
      type: 'div',
      classNames: [ 'mynah-ui-splash-loader-buttons' ],
      children: this.getSplashLoaderActions(this.props.splashScreenInitialStatus?.actions)
    });

    const initTabs = MynahUITabsStore.getInstance().getAllTabs();
    this.tabContentsWrapper = DomBuilder.getInstance().build({
      type: 'div',
      classNames: [ 'mynah-ui-tab-contents-wrapper' ],
      children: Object.keys(initTabs).slice(0, Config.getInstance().config.maxTabs).map((tabId: string) => {
        this.chatWrappers[tabId] = new ChatWrapper({
          tabId,
          onStopChatResponse: props.onStopChatResponse != null
            ? (tabId) => {
                if (props.onStopChatResponse != null) {
                  props.onStopChatResponse(tabId, this.getUserEventId());
                }
              }
            : undefined,
        });
        return this.chatWrappers[tabId].render;
      })
    });

    if (props.onSendFeedback !== undefined) {
      this.feedbackForm = new FeedbackForm();
    }

    this.sheet = new Sheet();

    if (Config.getInstance().config.maxTabs > 1) {
      this.tabsWrapper = new Tabs({
        onChange: (selectedTabId: string) => {
          this.focusToInput(selectedTabId);
          if (this.props.onTabChange !== undefined) {
            this.props.onTabChange(selectedTabId, this.getUserEventId());
          }
        },
        maxTabsTooltipDuration: Config.getInstance().config.maxTabsTooltipDuration,
        noMoreTabsTooltip: Config.getInstance().config.noMoreTabsTooltip,
        onBeforeTabRemove: (tabId): boolean => {
          if (props.onBeforeTabRemove !== undefined) {
            return props.onBeforeTabRemove(tabId, this.getUserEventId());
          }
          return true;
        }
      }).render;

      this.tabsWrapper.setAttribute('selected-tab', MynahUITabsStore.getInstance().getSelectedTabId());
    }

    this.render = DomBuilder.getInstance().createPortal(
      MynahPortalNames.WRAPPER,
      {
        type: 'div',
        attributes: {
          id: 'mynah-wrapper'
        },
        children: [
          this.tabsWrapper ?? '',
          ...(Config.getInstance().config.maxTabs > 1 ? [ new NoTabs().render ] : []),
          this.tabContentsWrapper,
        ]
      },
      'afterbegin'
    );
    this.splashLoader = DomBuilder.getInstance().createPortal(
      MynahPortalNames.LOADER,
      {
        type: 'div',
        classNames: [ 'mynah-ui-splash-loader-wrapper' ],
        children: [
          {
            type: 'div',
            classNames: [ 'mynah-ui-splash-loader-container' ],
            children: [
              new Icon({ icon: 'progress' }).render,
              this.splashLoaderText
            ]
          },
          this.splashLoaderActions
        ]
      },
      'beforeend');
    if (this.props.splashScreenInitialStatus?.visible === true) {
      this.splashLoader.addClass('visible');
    }

    MynahUITabsStore.getInstance().addListener('add', (tabId: string) => {
      this.chatWrappers[tabId] = new ChatWrapper({
        tabId,
        onStopChatResponse: props.onStopChatResponse != null
          ? (tabId) => {
              if (props.onStopChatResponse != null) {
                props.onStopChatResponse(tabId, this.getUserEventId());
              }
            }
          : undefined,
      });
      this.tabContentsWrapper.appendChild(this.chatWrappers[tabId].render);
      this.focusToInput(tabId);
      if (this.props.onTabAdd !== undefined) {
        this.props.onTabAdd(tabId, this.getUserEventId());
      }
    });
    MynahUITabsStore.getInstance().addListener('remove', (tabId: string) => {
      this.chatWrappers[tabId].render.remove();
      // eslint-disable-next-line @typescript-eslint/no-dynamic-delete
      delete this.chatWrappers[tabId];
      if (this.props.onTabRemove !== undefined) {
        this.props.onTabRemove(tabId, this.getUserEventId());
      }
    });

    this.addGlobalListeners();
    const tabId = MynahUITabsStore.getInstance().getSelectedTabId() ?? '';
    window.addEventListener('focus', () => {
      this.focusToInput(tabId);
    }, false);
    this.focusToInput(tabId);
    if (this.props.onReady !== undefined) {
      this.props.onReady();
    }
    if (Config.getInstance().config.enableSearchKeyboardShortcut === true) {
      document.addEventListener('keydown', (e) => {
        // Check for Command+F (Mac) or Ctrl+F (Windows/Linux)
        if ((e.metaKey || e.ctrlKey) && e.key === 'f') {
          cancelEvent(e);
          // Call the search shortcut handler with the current tab ID
          if (this.props.onSearchShortcut !== undefined) {
            this.props.onSearchShortcut(
              MynahUITabsStore.getInstance().getSelectedTabId(),
              this.getUserEventId());
          }
        }
      });
    }
  }

  private readonly getSplashLoaderActions = (actions?: Action[]): ExtendedHTMLElement[] => {
    return (actions ?? []).map(action => new Button({
      onClick: () => {
        this.props.onSplashLoaderActionClick?.(action, this.getUserEventId());
      },
      label: action.text,
      status: action.status,
      primary: action.status === 'primary',
      icon: action.icon != null ? new Icon({ icon: action.icon }).render : undefined,
      confirmation: action.confirmation,
      disabled: action.disabled,
      tooltip: action.description
    }).render);
  };

  private readonly getUserEventId = (): string => {
    this.lastEventId = generateUID();
    return this.lastEventId;
  };

  private readonly focusToInput = (tabId: string): void => {
    if (Config.getInstance().config.autoFocus) {
      MynahUIGlobalEvents.getInstance().dispatch(MynahEventNames.TAB_FOCUS, { tabId });
    }
  };

  private readonly addGlobalListeners = (): void => {
    MynahUIGlobalEvents.getInstance().addListener(MynahEventNames.CHAT_PROMPT, (data: { tabId: string; prompt: ChatPrompt }) => {
      if (this.props.onChatPrompt !== undefined) {
        this.props.onChatPrompt(data.tabId, data.prompt, this.getUserEventId());
      }
    });

    MynahUIGlobalEvents.getInstance().addListener(MynahEventNames.FOLLOW_UP_CLICKED, (data: {
      tabId: string;
      messageId: string;
      followUpOption: ChatItemAction;
    }) => {
      if (this.props.onFollowUpClicked !== undefined) {
        this.props.onFollowUpClicked(
          data.tabId,
          data.messageId,
          data.followUpOption,
          this.getUserEventId());
      }
    });

    MynahUIGlobalEvents.getInstance().addListener(MynahEventNames.CONTEXT_SELECTED, (data: {
      contextItem: QuickActionCommand;
      tabId: string;
      promptInputCallback: (insert: boolean) => void;
    }) => {
      data.promptInputCallback(this.props.onContextSelected === undefined || this.props.onContextSelected(data.contextItem, data.tabId, this.getUserEventId()));
    });

    MynahUIGlobalEvents.getInstance().addListener(MynahEventNames.FORM_MODIFIER_ENTER_PRESS, (data: {
      formData: Record<string, string>;
      tabId: string;
    }) => {
      if (this.props.onFormModifierEnterPress !== undefined) {
        this.props.onFormModifierEnterPress(data.formData, data.tabId, this.getUserEventId());
      }
    });

    MynahUIGlobalEvents.getInstance().addListener(MynahEventNames.FORM_TEXTUAL_ITEM_KEYPRESS, (data: {
      event: KeyboardEvent;
      formData: Record<string, string>;
      itemId: string;
      tabId: string;
      callback: (disableAll?: boolean) => void;
    }) => {
      if (this.props.onFormTextualItemKeyPress !== undefined) {
        data.callback(this.props.onFormTextualItemKeyPress(data.event, data.formData, data.itemId, data.tabId, this.getUserEventId()));
      }
    });

    MynahUIGlobalEvents.getInstance().addListener(MynahEventNames.FORM_CHANGE, (data: {
      formData: Record<string, string>;
      isValid: boolean;
      tabId: string;
    }) => {
      if (this.props.onFormChange !== undefined) {
        this.props.onFormChange(data.formData, data.isValid, data.tabId);
      }
    });

    MynahUIGlobalEvents.getInstance().addListener(MynahEventNames.BODY_ACTION_CLICKED, (data: {
      tabId: string;
      messageId: string;
      actionId: string;
      actionText?: string;
      formItemValues?: Record<string, string>;
    }) => {
      if (this.props.onInBodyButtonClicked !== undefined) {
        this.props.onInBodyButtonClicked(data.tabId, data.messageId, {
          id: data.actionId,
          text: data.actionText,
          formItemValues: data.formItemValues
        }, this.getUserEventId());
      }
    });

    MynahUIGlobalEvents.getInstance().addListener(MynahEventNames.QUICK_COMMAND_GROUP_ACTION_CLICK, (data: {
      tabId: string;
      actionId: string;
    }) => {
      if (this.props.onQuickCommandGroupActionClick !== undefined) {
        this.props.onQuickCommandGroupActionClick(data.tabId, {
          id: data.actionId,
        }, this.getUserEventId());
      }
    });

    MynahUIGlobalEvents.getInstance().addListener(MynahEventNames.TABBED_CONTENT_SWITCH, (data: {
      tabId: string;
      messageId: string;
      contentTabId: string;
    }) => {
      if (this.props.onTabbedContentTabChange != null) {
        this.props.onTabbedContentTabChange(data.tabId, data.messageId, data.contentTabId);
      }
    });

    MynahUIGlobalEvents.getInstance().addListener(MynahEventNames.PROMPT_PROGRESS_ACTION_CLICK, (data: {
      tabId: string;
      actionId: string;
      actionText?: string;
    }) => {
      if (this.props.onChatPromptProgressActionButtonClicked !== undefined) {
        this.props.onChatPromptProgressActionButtonClicked(data.tabId, {
          id: data.actionId,
          text: data.actionText
        }, this.getUserEventId());
      }
    });

    MynahUIGlobalEvents.getInstance().addListener(MynahEventNames.SHOW_MORE_WEB_RESULTS_CLICK, (data: { messageId: string }) => {
      if (this.props.onShowMoreWebResultsClick !== undefined) {
        this.props.onShowMoreWebResultsClick(
          MynahUITabsStore.getInstance().getSelectedTabId(),
          data.messageId,
          this.getUserEventId());
      }
    });

    MynahUIGlobalEvents.getInstance().addListener(MynahEventNames.FEEDBACK_SET, (feedbackData) => {
      if (this.props.onSendFeedback !== undefined) {
        this.props.onSendFeedback(
          MynahUITabsStore.getInstance().getSelectedTabId(),
          feedbackData,
          this.getUserEventId());
      }
    });

    MynahUIGlobalEvents.getInstance().addListener(MynahEventNames.CHAT_ITEM_ENGAGEMENT, (data: { engagement: Engagement; messageId: string }) => {
      if (this.props.onChatItemEngagement !== undefined) {
        this.props.onChatItemEngagement(MynahUITabsStore.getInstance().getSelectedTabId(), data.messageId, data.engagement);
      }
    });

    MynahUIGlobalEvents.getInstance().addListener(MynahEventNames.CODE_BLOCK_ACTION, (data) => {
      // TODO needs to be deprecated and followed through onCodeBlockActionClicked
      if (data.actionId === 'insert-to-cursor') {
        this.props.onCodeInsertToCursorPosition?.(
          MynahUITabsStore.getInstance().getSelectedTabId(),
          data.messageId,
          data.text,
          data.type,
          data.referenceTrackerInformation,
          this.getUserEventId(),
          data.codeBlockIndex,
          data.totalCodeBlocks,
        );
      }
      // TODO needs to be deprecated and followed through onCodeBlockActionClicked
      if (data.actionId === 'copy') {
        copyToClipboard(data.text, (): void => {
          this.props.onCopyCodeToClipboard?.(
            MynahUITabsStore.getInstance().getSelectedTabId(),
            data.messageId,
            data.text,
            data.type,
            data.referenceTrackerInformation,
            this.getUserEventId(),
            data.codeBlockIndex,
            data.totalCodeBlocks,
          );
        });
      }

      this.props.onCodeBlockActionClicked?.(
        MynahUITabsStore.getInstance().getSelectedTabId(),
        data.messageId,
        data.actionId,
        data.data,
        data.text,
        data.type,
        data.referenceTrackerInformation,
        this.getUserEventId(),
        data.codeBlockIndex,
        data.totalCodeBlocks,
      );
    });

    MynahUIGlobalEvents.getInstance().addListener(MynahEventNames.COPY_CODE_TO_CLIPBOARD, (data) => {
      if (this.props.onCopyCodeToClipboard !== undefined) {
        this.props.onCopyCodeToClipboard(
          MynahUITabsStore.getInstance().getSelectedTabId(),
          data.messageId,
          data.text,
          data.type,
          data.referenceTrackerInformation,
          this.getUserEventId(),
          data.codeBlockIndex,
          data.totalCodeBlocks,
        );
      }
    });

    MynahUIGlobalEvents.getInstance().addListener(MynahEventNames.SOURCE_LINK_CLICK, (data) => {
      if (this.props.onSourceLinkClick !== undefined) {
        this.props.onSourceLinkClick(
          MynahUITabsStore.getInstance().getSelectedTabId(),
          data.messageId,
          data.link,
          data.event,
          this.getUserEventId()
        );
      }
    });

    MynahUIGlobalEvents.getInstance().addListener(MynahEventNames.LINK_CLICK, (data) => {
      if (this.props.onLinkClick !== undefined) {
        this.props.onLinkClick(
          MynahUITabsStore.getInstance().getSelectedTabId(),
          data.messageId,
          data.link,
          data.event,
          this.getUserEventId()
        );
      }
    });
    MynahUIGlobalEvents.getInstance().addListener(MynahEventNames.FORM_LINK_CLICK, (data) => {
      if (this.props.onFormLinkClick !== undefined) {
        this.props.onFormLinkClick(
          data.link,
          data.event,
          this.getUserEventId()
        );
      }
    });
    MynahUIGlobalEvents.getInstance().addListener(MynahEventNames.INFO_LINK_CLICK, (data) => {
      if (this.props.onInfoLinkClick !== undefined) {
        this.props.onInfoLinkClick(
          MynahUITabsStore.getInstance().getSelectedTabId(),
          data.link,
          data.event,
          this.getUserEventId()
        );
      }
    });

    MynahUIGlobalEvents.getInstance().addListener(MynahEventNames.CARD_VOTE, (data) => {
      if (this.props.onVote !== undefined) {
        this.props.onVote(
          data.tabId,
          data.messageId,
          data.vote,
          this.getUserEventId()
        );
      }
    });

    MynahUIGlobalEvents.getInstance().addListener(MynahEventNames.RESET_STORE, (data: { tabId: string }) => {
      if (this.props.onResetStore !== undefined) {
        this.props.onResetStore(data.tabId);
      }
    });

    MynahUIGlobalEvents.getInstance().addListener(MynahEventNames.ROOT_FOCUS, (data: { focusState: boolean }) => {
      this.props.onFocusStateChanged?.(data.focusState);
    });

    MynahUIGlobalEvents.getInstance().addListener(MynahEventNames.FILE_CLICK, (data) => {
      if (this.props.onFileClick !== undefined) {
        this.props.onFileClick(
          data.tabId,
          data.filePath,
          data.deleted,
          data.messageId,
          this.getUserEventId(),
          data.fileDetails);
      }

      if (this.props.onOpenDiff !== undefined) {
        console.warn('onOpenDiff will be deprecated after v5.x.x. Please use MynahUIProps.onFileClick instead');
        this.props.onOpenDiff(
          data.tabId,
          data.filePath,
          data.deleted,
          data.messageId,
          this.getUserEventId());
      }
    });

    MynahUIGlobalEvents.getInstance().addListener(MynahEventNames.CARD_DISMISS, (data) => {
      this.props.onMessageDismiss?.(
        data.tabId,
        data.messageId,
        this.getUserEventId());
    });

    MynahUIGlobalEvents.getInstance().addListener(MynahEventNames.FILE_ACTION_CLICK, (data) => {
      if (this.props.onFileActionClick !== undefined) {
        this.props.onFileActionClick(
          data.tabId,
          data.messageId,
          data.filePath,
          data.actionName,
          this.getUserEventId());
      }
    });

    MynahUIGlobalEvents.getInstance().addListener(MynahEventNames.CUSTOM_FORM_ACTION_CLICK, (data) => {
      if (this.props.onCustomFormAction !== undefined) {
        this.props.onCustomFormAction(data.tabId, data, this.getUserEventId());
      }
    });

    MynahUIGlobalEvents.getInstance().addListener(MynahEventNames.PROMPT_INPUT_OPTIONS_CHANGE, (data) => {
      this.props.onPromptInputOptionChange?.(data.tabId, data.optionsValues, this.getUserEventId());
    });

    MynahUIGlobalEvents.getInstance().addListener(MynahEventNames.DROPDOWN_OPTION_CHANGE, (data) => {
      this.props.onDropDownOptionChange?.(data.tabId, data.messageId, data.value);
    });

    MynahUIGlobalEvents.getInstance().addListener(MynahEventNames.DROPDOWN_LINK_CLICK, (data) => {
      this.props.onDropDownLinkClick?.(data.tabId, data.actionId, data.destination);
    });

    MynahUIGlobalEvents.getInstance().addListener(MynahEventNames.TOP_BAR_ITEM_ADD, (data) => {
      this.props.onPromptTopBarItemAdded?.(data.tabId, data.contextItem, this.getUserEventId());
    });
    MynahUIGlobalEvents.getInstance().addListener(MynahEventNames.TOP_BAR_ITEM_REMOVE, (data) => {
      this.props.onPromptTopBarItemRemoved?.(data.tabId, data.contextItem, this.getUserEventId());
    });
    MynahUIGlobalEvents.getInstance().addListener(MynahEventNames.TOP_BAR_BUTTON_CLICK, (data) => {
      this.props.onPromptTopBarButtonClick?.(data.tabId, data.button, this.getUserEventId());
    });

    MynahUIGlobalEvents.getInstance().addListener(MynahEventNames.PROMPT_INPUT_BUTTON_CLICK, (data) => {
      this.props.onPromptInputButtonClick?.(data.tabId, data.buttonId, this.getUserEventId());
    });

    MynahUIGlobalEvents.getInstance().addListener(MynahEventNames.TAB_BAR_BUTTON_CLICK, (data) => {
      if (this.props.onTabBarButtonClick !== undefined) {
        this.props.onTabBarButtonClick(data.tabId, data.buttonId, this.getUserEventId());
      }
    });

    MynahUIGlobalEvents.getInstance().addListener(MynahEventNames.OPEN_FILE_SYSTEM, (data) => {
      this.props.onOpenFileDialogClick?.(data.tabId, data.type, data.insertPosition);
    });

    MynahUIGlobalEvents.getInstance().addListener(MynahEventNames.FILES_DROPPED, (data) => {
      this.props.onFilesDropped?.(data.tabId, data.files, data.insertPosition);
    });
  };

  public addToUserPrompt = (tabId: string, attachmentContent: string, type?: PromptAttachmentType): void => {
    if (Config.getInstance().config.showPromptField && MynahUITabsStore.getInstance().getTab(tabId) !== null) {
      this.chatWrappers[tabId].addAttachmentToPrompt(attachmentContent, type);
    }
  };

  /**
   * Adds a new item to the chat window
   * @param tabId Corresponding tab ID.
   * @param answer ChatItem object.
   */
  public addChatItem = (tabId: string, chatItem: ChatItem): void => {
    if (MynahUITabsStore.getInstance().getTab(tabId) !== null) {
      MynahUIGlobalEvents.getInstance().dispatch(MynahEventNames.CHAT_ITEM_ADD, { tabId, chatItem });
      MynahUITabsStore.getInstance().getTabDataStore(tabId).updateStore({
        chatItems: [
          ...MynahUITabsStore.getInstance().getTabDataStore(tabId).getValue('chatItems'),
          chatItem
        ]
      });
    }
  };

  public addCustomContextToPrompt = (tabId: string, contextItem: QuickActionCommand[], insertPosition?: number): void => {
    if (MynahUITabsStore.getInstance().getTab(tabId) !== null) {
      // Get the current trigger source from the chat wrapper
      const chatWrapper = this.chatWrappers[tabId];
      const currentTriggerSource = chatWrapper?.getCurrentTriggerSource?.() ?? 'prompt-input';

      if (currentTriggerSource === 'top-bar') {
        // If triggered from top bar, add to top bar instead
        contextItem.forEach(item => {
          MynahUIGlobalEvents.getInstance().dispatch(MynahEventNames.CONTEXT_PINNED, { tabId, contextItem: item });
        });
      } else {
        // Update the data store's customContextCommand field
        const currentCustomContext = MynahUITabsStore.getInstance().getTabDataStore(tabId).getValue('customContextCommand') as QuickActionCommand[] ?? [];
        MynahUITabsStore.getInstance().getTabDataStore(tabId).updateStore({
          customContextCommand: [ ...currentCustomContext, ...contextItem ]
        });

        // Dispatch the event for UI updates
        MynahUIGlobalEvents.getInstance().dispatch(MynahEventNames.ADD_CUSTOM_CONTEXT, { tabId, contextCommands: contextItem, insertPosition });
      }

      // Dispatch event to signal context has been inserted
      MynahUIGlobalEvents.getInstance().dispatch(MynahEventNames.RESET_TOP_BAR_CLICKED, { tabId });
    }
  };

  /**
   * Updates the last ChatItemType.ANSWER_STREAM chat item
   * @param tabId Corresponding tab ID.
   * @param updateWith ChatItem object to update with.
   */
  public updateLastChatAnswer = (tabId: string, updateWith: Partial<ChatItem>): void => {
    if (MynahUITabsStore.getInstance().getTab(tabId) != null) {
      if (this.chatWrappers[tabId].getLastStreamingMessageId() != null) {
        this.chatWrappers[tabId].updateLastChatAnswer(updateWith);
      } else {
        // We're assuming consumer shouldn't try to update last chat item if it is not a streaming one
        // However, to be on the safe side, if there is no streaming card available, we're adding one.
        this.addChatItem(tabId, {
          type: ChatItemType.ANSWER_STREAM,
          body: '',
          messageId: generateUID(),
        });
        this.chatWrappers[tabId].updateLastChatAnswer(updateWith);
      }
    }
  };

  /**
   * Updates the chat item with the given messageId
   * @param tabId Corresponding tab ID.
   * @param messageId Corresponding tab ID.
   * @param updateWith ChatItem object to update with.
   */
  public updateChatAnswerWithMessageId = (tabId: string, messageId: string, updateWith: Partial<ChatItem>): void => {
    if (MynahUITabsStore.getInstance().getTab(tabId) !== null) {
      this.chatWrappers[tabId].updateChatAnswerWithMessageId(messageId, updateWith);
    }
  };

  /**
   * Serialize all (non-empty) chat messages in a tab into a string
   * @param tabId Corresponding tab ID.
   * @param format Whether to serialize to markdown or HTML format
   */
  public serializeChat = (tabId: string, format: 'markdown' | 'html'): string => {
    if (format === 'markdown') {
      return serializeMarkdown(tabId);
    }
    return serializeHtml(tabId);
  };

  /**
   * Converts a card to an ANSWER if it is an ANSWER_STREAM
   * @param tabId Corresponding tab ID.
   * @param messageId Corresponding tab ID.
   * @param updateWith Optional, if you like update the card while converting it to
   * a normal ANSWER instead of a stream one, you can send a ChatItem object to update with.
   */
  public endMessageStream = (tabId: string, messageId: string, updateWith?: Partial<ChatItem>): CardRenderDetails => {
    if (MynahUITabsStore.getInstance().getTab(tabId) !== null) {
      const chatMessage = this.chatWrappers[tabId].getChatItem(messageId);
      if (chatMessage != null && ![ ChatItemType.AI_PROMPT, ChatItemType.PROMPT, ChatItemType.SYSTEM_PROMPT ].includes(chatMessage.chatItem.type)) {
        this.chatWrappers[tabId].endStreamWithMessageId(messageId, {
          type: ChatItemType.ANSWER,
          ...updateWith
        });
        return chatMessage.renderDetails;
      }
    }
    return {
      totalNumberOfCodeBlocks: 0
    };
  };

  /**
   * If exists, switch to a different tab
   * @param tabId Tab ID to switch to
   * @param eventId last action's user event ID passed from an event binded to mynahUI.
   * Without user intent you cannot switch to a different tab
   */
  public selectTab = (tabId: string, eventId?: string): void => {
    // TODO: until we find a way to confirm the events from the consumer as events,
    // eventId === this.lastEventId: This check will be removed
    if (MynahUITabsStore.getInstance().getTab(tabId) !== null) {
      MynahUITabsStore.getInstance().selectTab(tabId);
    }
  };

  /**
   * If exists, close the given tab
   * @param tabId Tab ID to switch to
   * @param eventId last action's user event ID passed from an event binded to mynahUI.
   * Without user intent you cannot switch to a different tab
   */
  public removeTab = (tabId: string, eventId: string): void => {
    if (eventId === this.lastEventId && MynahUITabsStore.getInstance().getTab(tabId) !== null) {
      MynahUITabsStore.getInstance().removeTab(tabId);
    }
  };

  /**
   * Updates only the UI with the given data for the given tab
   * Send tab id as an empty string to open a new tab!
   * If max tabs reached, will not return tabId
   * @param data A full or partial set of data with values.
   */
  public updateStore = (tabId: string | '', data: MynahUIDataModel): string | undefined => {
    if (tabId === '') {
      return MynahUITabsStore.getInstance().addTab({ store: { ...data } });
    } else if (MynahUITabsStore.getInstance().getTab(tabId) !== null) {
      MynahUITabsStore.getInstance().updateTab(tabId, { store: { ...data } });
    }
    return tabId;
  };

  /**
   * Updates defaults of the tab store
   * @param defaults MynahUITabStoreTab
   */
  public updateTabDefaults = (defaults: MynahUITabStoreTab): void => {
    MynahUITabsStore.getInstance().updateTabDefaults(defaults);
  };

  /**
   * Updates defaults of the tab store
   * @param defaults MynahUITabStoreTab
   */
  public getTabDefaults = (): MynahUITabStoreTab => {
    return MynahUITabsStore.getInstance().getTabDefaults();
  };

  /**
   * This function returns the selected tab id if there is any, otherwise returns undefined
   * @returns string selectedTabId or undefined
   */
  public getSelectedTabId = (): string | undefined => {
    const selectedTabId = MynahUITabsStore.getInstance().getSelectedTabId();
    return selectedTabId === '' ? undefined : selectedTabId;
  };

  /**
   * Returns all tabs with their store information
   * @returns string selectedTabId or undefined
   */
  public getAllTabs = (): MynahUITabStoreModel => MynahUITabsStore.getInstance().getAllTabs();

  public getTabData = (tabId: string): MynahUIDataStore => MynahUITabsStore.getInstance().getTabDataStore(tabId);

  /**
   * Gets the current prompt input text for a tab (the text the user is typing)
   * @param tabId The tab ID to get the prompt input text for
   * @returns The current prompt input text, or empty string if tab not found
   */
  public getPromptInputText = (tabId: string): string => {
    const chatWrapper = this.chatWrappers[tabId];
    if (chatWrapper != null) {
      return chatWrapper.getPromptInputText();
    }
    return '';
  };

  /**
   * Sets the drag overlay visibility for a specific tab
   * @param tabId The tab ID to set the drag overlay visibility for
   * @param visible Whether the drag overlay should be visible
   */
  public setDragOverlayVisible = (tabId: string, visible: boolean): void => {
    if (this.chatWrappers[tabId] !== null) {
      this.chatWrappers[tabId].setDragOverlayVisible(visible);
    }
  };

  /**
   * Programmatically resets topBarClicked for the specified tab by dispatching a RESET_TOP_BAR_CLICKED event.
   * @param tabId The tab ID
   */
  public resetTopBarClicked = (tabId: string): void => {
    MynahUIGlobalEvents.getInstance().dispatch(MynahEventNames.RESET_TOP_BAR_CLICKED, { tabId });
  };

  /**
   * Toggles the visibility of the splash loader screen
   */
  public toggleSplashLoader = (visible: boolean, text?: string, actions?: Action[]): void => {
    if (visible) {
      this.splashLoader.addClass('visible');
    } else {
      this.splashLoader.removeClass('visible');
    }

    if (text != null) {
      this.splashLoaderText.update({
        innerHTML: parseMarkdown(text, { includeLineBreaks: true })
      });
    }

    this.splashLoaderActions.clear();
    this.splashLoaderActions.update({
      children: this.getSplashLoaderActions(actions)
    });
  };

  /**
   * Simply creates and shows a notification
   * @param props NotificationProps
   */
  public notify = (props: {
    /**
     * -1 for infinite
     */
    duration?: number;
    type?: NotificationType;
    title?: string;
    content: string;
    onNotificationClick?: (eventId: string) => void;
    onNotificationHide?: (eventId: string) => void;
  }): void => {
    new Notification({
      ...props,
      onNotificationClick: (props.onNotificationClick != null)
        ? () => {
            if (props.onNotificationClick != null) {
              props.onNotificationClick(this.getUserEventId());
            }
          }
        : undefined,
      onNotificationHide: (props.onNotificationHide != null)
        ? () => {
            if (props.onNotificationHide != null) {
              props.onNotificationHide(this.getUserEventId());
            }
          }
        : undefined,
    }).notify();
  };

  /**
   * Simply creates and shows a custom form
   */
  public showCustomForm = (
    tabId: string,
    formItems?: ChatItemFormItem[],
    buttons?: ChatItemButton[],
    title?: string,
    description?: string): void => {
    MynahUIGlobalEvents.getInstance().dispatch(MynahEventNames.SHOW_FEEDBACK_FORM, {
      tabId,
      customFormData: {
        title,
        description,
        buttons,
        formItems
      },
    });
  };

  public openTopBarButtonOverlay = (data: TopBarButtonOverlayProps,
  ): {
      update: (data: DetailedList) => void;
      close: () => void;

    } => {
    if (data.tabId != null) {
      this.chatWrappers[data.tabId].openTopBarButtonItemOverlay(data);
    }
    return {
      update: this.chatWrappers[data.tabId].updateTopBarButtonItemOverlay,
      close: this.chatWrappers[data.tabId].closeTopBarButtonItemOverlay
    };
  };

  public openDetailedList = (
    data: DetailedListSheetProps,
    showBackButton?: boolean,
  ): {
      update: (data: DetailedList, showBackButton?: boolean) => void;
      close: () => void;
      changeTarget: (direction: 'up' | 'down', snapOnLastAndFirst?: boolean) => void;
      getTargetElementId: () => string | undefined;
    } => {
    const detailedListSheet = new DetailedListSheet({
      detailedList: data.detailedList,
      events: data.events
    });
    detailedListSheet.open(showBackButton);

    const getTargetElementId = (): string | undefined => {
      const targetElement = detailedListSheet.detailedListWrapper.getTargetElement();
      return targetElement?.id ?? undefined;
    };
    return {
      update: detailedListSheet.update,
      close: detailedListSheet.close,
      changeTarget: detailedListSheet.detailedListWrapper.changeTarget,
      getTargetElementId
    };
  };

  public destroy = (): void => {
    // Destroy all chat wrappers
    Object.values(this.chatWrappers).forEach(chatWrapper => {
      chatWrapper.destroy();
    });

    Config.getInstance().destroy();
    MynahUITabsStore.getInstance().destroy();
    MynahUIGlobalEvents.getInstance().destroy();
    DomBuilder.getInstance().destroy();
  };
}
