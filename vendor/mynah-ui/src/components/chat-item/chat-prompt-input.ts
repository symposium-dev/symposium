/*!
 * Copyright 2022 Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

import { DomBuilder, ExtendedHTMLElement } from '../../helper/dom';
import { ChatItemButton, ChatPrompt, DetailedList, FilterOption, KeyMap, MynahEventNames, PromptAttachmentType, QuickActionCommand, QuickActionCommandGroup, QuickActionCommandsHeader } from '../../static';
import { TitleDescriptionWithIcon } from '../title-description-with-icon';
import { MynahUIGlobalEvents, cancelEvent } from '../../helper/events';
import { Overlay, OverlayHorizontalDirection, OverlayVerticalDirection } from '../overlay';
import { MynahUITabsStore } from '../../helper/tabs-store';
import escapeHTML from 'escape-html';
import { ChatPromptInputCommand } from './chat-prompt-input-command';
import { PromptAttachment } from './prompt-input/prompt-attachment';
import { PromptInputSendButton } from './prompt-input/prompt-input-send-button';
import { PromptTextInput } from './prompt-input/prompt-text-input';
import { Config } from '../../helper/config';
import testIds from '../../helper/test-ids';
import { PromptInputProgress } from './prompt-input/prompt-progress';
import { CardBody } from '../card/card-body';
import {
  convertDetailedListItemToQuickActionCommand,
  convertQuickActionCommandGroupsToDetailedListGroups,
  filterQuickPickItems,
  MARK_CLOSE,
  MARK_OPEN
} from '../../helper/quick-pick-data-handler';
import { DetailedListWrapper } from '../detailed-list/detailed-list';
import { PromptOptions } from './prompt-input/prompt-options';
import { PromptInputStopButton } from './prompt-input/prompt-input-stop-button';
import { PromptTopBar } from './prompt-input/prompt-top-bar/prompt-top-bar';
import { TopBarButtonOverlayProps } from './prompt-input/prompt-top-bar/top-bar-button';
import { Button } from '../button';
import { Icon, MynahIcons } from '../icon';

// 96 extra is added as a threshold to allow for attachments
// We ignore this for the textual character limit
export const MAX_USER_INPUT_THRESHOLD = 96;
export const MAX_USER_INPUT = (): number => {
  return Config.getInstance().config.maxUserInput - MAX_USER_INPUT_THRESHOLD;
};

// The amount of characters in the prompt input necessary for the warning to show
export const INPUT_LENGTH_WARNING_THRESHOLD = (): number => {
  return Config.getInstance().config.userInputLengthWarningThreshold;
};

export interface ChatPromptInputProps {
  tabId: string;
  onStopChatResponse?: (tabId: string) => void;
}

interface UserPrompt {
  inputText: string;
  codeAttachment: string;
}

export class ChatPromptInput {
  render: ExtendedHTMLElement;
  private readonly props: ChatPromptInputProps;
  private readonly attachmentWrapper: ExtendedHTMLElement;
  private readonly promptTextInput: PromptTextInput;
  private readonly contextSelectorButton: Button;
  private readonly promptTextInputCommand: ChatPromptInputCommand;
  private readonly sendButton: PromptInputSendButton;
  private readonly stopButton: PromptInputStopButton;
  private readonly progressIndicator: PromptInputProgress;
  private readonly promptAttachment: PromptAttachment;
  private readonly promptOptions: PromptOptions;
  private readonly promptTopBar: PromptTopBar;
  private readonly chatPrompt: ExtendedHTMLElement;
  private quickPickItemsSelectorContainer: DetailedListWrapper | null;
  private promptTextInputLabel: ExtendedHTMLElement;
  private remainingCharsOverlay: Overlay | null;
  /**
   * Preserves cursor position when `@` key is pressed
   */
  private quickPickTriggerIndex: number;
  /**
   * Preserves selection range when `@` key is pressed
   */
  private quickPickTriggerRange?: Range;
  private quickPickType: 'quick-action' | 'context';
  private quickPickItemGroups: QuickActionCommandGroup[];
  private topBarTitleClicked: boolean = false;
  private filteredQuickPickItemGroups: QuickActionCommandGroup[];
  private searchTerm: string = '';
  private quickPick: Overlay;
  private quickPickOpen: boolean = false;
  private selectedCommand: string = '';
  private readonly userPromptHistory: UserPrompt[] = [];
  private userPromptHistoryIndex: number = -1;
  private lastUnsentUserPrompt: UserPrompt;
  private readonly markerRemovalRegex = new RegExp(`${MARK_OPEN}|${MARK_CLOSE}`, 'g');
  constructor (props: ChatPromptInputProps) {
    this.props = props;
    this.promptTextInputCommand = new ChatPromptInputCommand({
      command: '',
      onRemoveClick: () => {
        this.selectedCommand = '';
        this.promptTextInputCommand.setCommand('');
      }
    });

    this.promptTextInput = new PromptTextInput({
      initMaxLength: MAX_USER_INPUT(),
      tabId: this.props.tabId,
      children: [ this.promptTextInputCommand.render ],
      onKeydown: this.handleInputKeydown,
      onInput: () => this.updateAvailableCharactersIndicator(),
      onFocus: () => {
        this.render.addClass('input-has-focus');
        this.handleInputFocus();
      },
      onBlur: () => {
        if (this.render.hasClass('awaits-confirmation')) {
          this.promptTextInputCommand.setCommand('');
          this.selectedCommand = '';
          this.promptTextInput.updateTextInputPlaceholder(MynahUITabsStore.getInstance().getTabDataStore(this.props.tabId).getValue('promptInputPlaceholder'));
          this.promptTextInput.updateTextInputMaxLength(Config.getInstance().config.maxUserInput);
          if (Config.getInstance().config.autoFocus) {
            this.promptTextInput.focus();
          }
          this.render.removeClass('awaits-confirmation');
        }
        this.render.removeClass('input-has-focus');
        this.remainingCharsOverlay?.close();
      }
    });
    const initText = MynahUITabsStore.getInstance().getTabDataStore(this.props.tabId).getValue('promptInputText');
    if (initText != null && initText.trim() !== '') {
      this.promptTextInput.updateTextInputValue(initText);
    }
    this.sendButton = new PromptInputSendButton({
      tabId: this.props.tabId,
      onClick: () => {
        this.sendPrompt();
      },
    });
    this.stopButton = new PromptInputStopButton({
      tabId: this.props.tabId,
      onClick: () => {
        if (this.props.onStopChatResponse != null) {
          this.props.onStopChatResponse(this.props.tabId);
        }
      },
    });
    this.progressIndicator = new PromptInputProgress({
      tabId: this.props.tabId,
    });

    this.promptAttachment = new PromptAttachment({
      tabId: this.props.tabId,
    });

    this.promptOptions = new PromptOptions({
      filterOptions: MynahUITabsStore.getInstance().getTabDataStore(this.props.tabId).getValue('promptInputOptions'),
      buttons: MynahUITabsStore.getInstance().getTabDataStore(this.props.tabId).getValue('promptInputButtons'),
      onFiltersChange: (formData) => {
        MynahUIGlobalEvents.getInstance().dispatch(MynahEventNames.PROMPT_INPUT_OPTIONS_CHANGE, {
          tabId: this.props.tabId,
          optionsValues: formData
        });
      },
      onButtonClick: (buttonId) => {
        MynahUIGlobalEvents.getInstance().dispatch(MynahEventNames.PROMPT_INPUT_BUTTON_CLICK, {
          tabId: this.props.tabId,
          buttonId
        });
      }
    });

    this.promptTopBar = new PromptTopBar({
      tabId: this.props.tabId,
      title: MynahUITabsStore.getInstance().getTabDataStore(this.props.tabId).getValue('promptTopBarTitle'),
      topBarButton: MynahUITabsStore.getInstance().getTabDataStore(this.props.tabId).getValue('promptTopBarButton'),
      contextItems: MynahUITabsStore.getInstance().getTabDataStore(this.props.tabId).getValue('promptTopBarContextItems'),
      onTopBarTitleClick: () => {
        this.onContextSelectorButtonClick(true);
      },
      onContextItemAdd: (contextItem: QuickActionCommand) => {
        MynahUIGlobalEvents.getInstance().dispatch(MynahEventNames.TOP_BAR_ITEM_ADD, {
          tabId: this.props.tabId,
          contextItem
        });
      },
      onContextItemRemove: (contextItem: QuickActionCommand) => {
        MynahUIGlobalEvents.getInstance().dispatch(MynahEventNames.TOP_BAR_ITEM_REMOVE, {
          tabId: this.props.tabId,
          contextItem
        });
      },
      onTopBarButtonClick: (button: ChatItemButton) => {
        MynahUIGlobalEvents.getInstance().dispatch(MynahEventNames.TOP_BAR_BUTTON_CLICK, {
          tabId: this.props.tabId,
          button
        });
      }

    });

    this.attachmentWrapper = DomBuilder.getInstance().build({
      type: 'div',
      testId: testIds.prompt.attachmentWrapper,
      classNames: [ 'mynah-chat-prompt-attachment-wrapper' ],
      children: [
        this.promptAttachment.render
      ]
    });

    const noContextCommands = (MynahUITabsStore.getInstance().getTabDataStore(this.props.tabId).getValue('contextCommands') as QuickActionCommandGroup[] ?? []).length === 0;

    this.contextSelectorButton = new Button({
      icon: new Icon({ icon: MynahIcons.AT }).render,
      status: 'clear',
      disabled: noContextCommands,
      classNames: (noContextCommands || !this.promptTopBar.isHidden()) ? [ 'hidden' ] : [],
      primary: false,
      onClick: () => {
        this.onContextSelectorButtonClick();
      },
    });

    MynahUITabsStore.getInstance().addListenerToDataStore(this.props.tabId, 'contextCommands', (contextCommands) => {
      if (contextCommands?.length > 0 && this.promptTopBar.isHidden()) {
        this.contextSelectorButton.setEnabled(true);
        this.contextSelectorButton.render.removeClass('hidden');
      } else {
        this.contextSelectorButton.setEnabled(false);
        this.contextSelectorButton.render.addClass('hidden');
      }
    });
    this.chatPrompt = DomBuilder.getInstance().build({
      type: 'div',
      classNames: [ 'mynah-chat-prompt' ],
      children: [
        this.progressIndicator.render,
        this.chatPrompt,
        {
          type: 'div',
          classNames: [ 'mynah-chat-prompt-input-wrapper' ],
          children: [
            this.promptTopBar.render,
            this.promptTextInput.render,
            {
              type: 'div',
              classNames: [ 'mynah-chat-prompt-button-wrapper' ],
              children: [
                this.promptOptions.render,
                this.contextSelectorButton.render,
                this.stopButton.render,
                this.sendButton.render,
              ]
            },
          ]
        },
        this.attachmentWrapper,
      ]
    });

    MynahUITabsStore.getInstance().addListenerToDataStore(this.props.tabId, 'promptInputText', (promptInputText: string) => {
      if (this.promptTextInput.getTextInputValue() !== promptInputText) {
        this.promptTextInput.clear();
        this.promptTextInput.updateTextInputValue(promptInputText);
        setTimeout(() => {
          this.promptTextInput.focus();
        }, 750);
      }
    });
    MynahUITabsStore.getInstance().addListenerToDataStore(this.props.tabId, 'promptInputOptions', (newFilterOptions: FilterOption[]) => {
      this.promptOptions.update(newFilterOptions);
    });
    MynahUITabsStore.getInstance().addListenerToDataStore(this.props.tabId, 'promptInputButtons', (newButtons: ChatItemButton[]) => {
      this.promptOptions.update(undefined, newButtons);
    });

    MynahUITabsStore.getInstance().addListenerToDataStore(this.props.tabId, 'promptTopBarContextItems', (newCommands: QuickActionCommand[]) => {
      this.promptTopBar.update({ contextItems: newCommands });
    });
    MynahUITabsStore.getInstance().addListenerToDataStore(this.props.tabId, 'promptTopBarTitle', (newTitle: string) => {
      this.promptTopBar.update({ title: newTitle });

      if (!this.promptTopBar.isHidden()) {
        this.contextSelectorButton.setEnabled(false);
        this.contextSelectorButton.render.addClass('hidden');
      }
    });
    MynahUITabsStore.getInstance().addListenerToDataStore(this.props.tabId, 'promptTopBarButton', (newButton: ChatItemButton) => {
      this.promptTopBar.update({ topBarButton: newButton });
    });

    MynahUITabsStore.getInstance().addListenerToDataStore(this.props.tabId, 'promptInputLabel', (promptInputLabel: string) => {
      const newDetails = this.getPromptInputTextLabel(promptInputLabel);
      if (this.promptTextInputLabel != null) {
        this.promptTextInputLabel.replaceWith(newDetails);
      } else {
        this.promptTextInputLabel = newDetails;
      }
    });

    MynahUITabsStore.getInstance().addListenerToDataStore(this.props.tabId, 'promptInputVisible', (promptInputVisible?: boolean) => {
      if (promptInputVisible === false) {
        this.render.addClass('hidden');
      } else {
        this.render.removeClass('hidden');
      }
    });

    this.promptTextInputLabel = this.getPromptInputTextLabel(MynahUITabsStore.getInstance().getTabDataStore(this.props.tabId).getValue('promptInputLabel'));

    this.render = DomBuilder.getInstance().build({
      type: 'div',
      testId: testIds.prompt.wrapper,
      classNames: [ 'mynah-chat-prompt-wrapper', MynahUITabsStore.getInstance().getTabDataStore(props.tabId).getValue('promptInputVisible') === false ? 'hidden' : '' ],
      children: [
        this.promptTextInputLabel,
        this.chatPrompt
      ],
    });

    MynahUIGlobalEvents.getInstance().addListener(MynahEventNames.ADD_ATTACHMENT, (data: {
      textToAdd?: string;
      tabId?: string;
      type?: PromptAttachmentType;
    }) => {
      if (this.props.tabId === data.tabId) {
        // Code snippet will have a limit of MAX_USER_INPUT - MAX_USER_INPUT_THRESHOLD - current prompt text length
        // If exceeding that, we will crop it
        const textInputLength = this.promptTextInput.getTextInputValue().trim().length;
        const currentSelectedCodeMaxLength = (MAX_USER_INPUT()) - textInputLength;
        const croppedAttachmentContent = (data.textToAdd ?? '')?.slice(0, currentSelectedCodeMaxLength);
        this.promptAttachment.updateAttachment(croppedAttachmentContent, data.type);
        // Also update the limit on prompt text given the selected code
        this.promptTextInput.updateTextInputMaxLength(Math.max(MAX_USER_INPUT_THRESHOLD, (MAX_USER_INPUT() - croppedAttachmentContent.length)));
        this.updateAvailableCharactersIndicator();

        // When code is attached, focus to the input with a delay
        // Delay is necessary for the render updates
        setTimeout(() => {
          this.promptTextInput.focus();
        }, 100);
      }
    });

    MynahUIGlobalEvents.getInstance().addListener(MynahEventNames.REMOVE_ATTACHMENT, () => {
      this.promptTextInput.updateTextInputMaxLength(MAX_USER_INPUT());
      this.promptAttachment.clear();
      // Update the limit on prompt text given the selected code
      this.updateAvailableCharactersIndicator();
    });

    MynahUIGlobalEvents.getInstance().addListener(MynahEventNames.RESET_TOP_BAR_CLICKED, (data: { tabId: string }) => {
      if (this.props.tabId === data.tabId) {
        // Reset trigger source to prompt-input after context is inserted
        this.topBarTitleClicked = false;
      }
    });
  }

  private readonly onContextSelectorButtonClick = (topBarTitleClicked?: boolean): void => {
    this.searchTerm = '';
    this.quickPickType = 'context';
    this.quickPickItemGroups = (MynahUITabsStore.getInstance().getTabDataStore(this.props.tabId).getValue('contextCommands') as QuickActionCommandGroup[]) ?? [];
    this.quickPickTriggerIndex = this.promptTextInput.getCursorPos();
    this.quickPickTriggerRange = window.getSelection()?.getRangeAt(0);
    this.filteredQuickPickItemGroups = [ ...this.quickPickItemGroups ];
    if (topBarTitleClicked !== true) { this.promptTextInput.insertEndSpace(); }
    this.openQuickPick(topBarTitleClicked);
  };

  private readonly updateAvailableCharactersIndicator = (): void => {
    const characterAmount = MAX_USER_INPUT() - Math.max(0, (this.promptTextInput.promptTextInputMaxLength - this.promptTextInput.getTextInputValue().trim().length));
    const charTextElm = DomBuilder.getInstance().build({
      type: 'span',
      classNames: [ 'mynah-chat-prompt-chars-indicator' ],
      innerHTML: `${characterAmount}/${MAX_USER_INPUT()}`,
    });

    // Re(render) if the overlay is not in the DOM, else update
    if (this.remainingCharsOverlay == null || this.remainingCharsOverlay.render.parentNode == null) {
      this.remainingCharsOverlay = new Overlay({
        testId: testIds.prompt.remainingCharsIndicator,
        background: true,
        closeOnOutsideClick: false,
        referenceElement: this.chatPrompt,
        dimOutside: false,
        verticalDirection: OverlayVerticalDirection.TO_BOTTOM,
        horizontalDirection: OverlayHorizontalDirection.END_TO_LEFT,
        children: [
          charTextElm
        ],
      });
    } else {
      this.remainingCharsOverlay.updateContent([
        charTextElm
      ]);
    }

    // Set the visibility based on whether the threshold is hit
    if (characterAmount >= INPUT_LENGTH_WARNING_THRESHOLD()) {
      this.remainingCharsOverlay.toggleHidden(false);
    } else {
      this.remainingCharsOverlay.toggleHidden(true);
    }
  };

  private readonly handleInputKeydown = (e: KeyboardEvent): void => {
    const navigationalKeys = [ KeyMap.ARROW_UP, KeyMap.ARROW_DOWN ] as string[];

    if (e.key === KeyMap.ESCAPE && this.render.hasClass('awaits-confirmation')) {
      this.promptTextInput.blur();
    }
    if (!this.quickPickOpen) {
      if (e.key === KeyMap.BACKSPACE || e.key === KeyMap.DELETE) {
        if (this.selectedCommand !== '' && this.promptTextInput.getTextInputValue() === '') {
          cancelEvent(e);
          this.clearTextArea(true);
        }
      } else if (e.key === KeyMap.ENTER &&
        ((!e.isComposing && !e.shiftKey && !e.ctrlKey) ||
        (e.isComposing && (e.shiftKey)))) {
        cancelEvent(e);
        this.sendPrompt();
      } else if (
        (this.selectedCommand === '' && e.key === KeyMap.SLASH && this.promptTextInput.getTextInputValue() === '') ||
        (e.key === KeyMap.AT && this.promptTextInput.promptTextInputMaxLength > 0)
      ) {
        const quickPickContextItems = (MynahUITabsStore.getInstance().getTabDataStore(this.props.tabId).getValue('contextCommands') as QuickActionCommandGroup[]) ?? [];
        const quickPickCommandItems = (MynahUITabsStore.getInstance().getTabDataStore(this.props.tabId).getValue('quickActionCommands') as QuickActionCommandGroup[]) ?? [];
        this.searchTerm = '';
        this.quickPickType = e.key === KeyMap.AT ? 'context' : 'quick-action';
        this.quickPickItemGroups = this.quickPickType === 'context' ? quickPickContextItems : quickPickCommandItems;
        this.quickPickTriggerRange = window.getSelection()?.getRangeAt(0);
        this.quickPickTriggerIndex = this.quickPickType === 'context' ? this.promptTextInput.getCursorPos() : 1;
        this.filteredQuickPickItemGroups = [ ...this.quickPickItemGroups ];
        this.openQuickPick();
      } else if (navigationalKeys.includes(e.key)) {
        const cursorPosition = this.promptTextInput.getCursorPosition();

        // Only enter history navigation if:
        // 1. Going up and there's history to go up to, or we're at the beginning of history navigation
        // 2. Going down and we're not already at the bottom of history
        const shouldNavigateUp = cursorPosition.isAtTheBeginning && e.key === KeyMap.ARROW_UP &&
                                (this.userPromptHistoryIndex > 0 || this.userPromptHistoryIndex === -1) &&
                                this.userPromptHistory.length > 0;
        const shouldNavigateDown = cursorPosition.isAtTheEnd && e.key === KeyMap.ARROW_DOWN &&
                                  this.userPromptHistoryIndex !== -1 &&
                                  this.userPromptHistoryIndex < this.userPromptHistory.length;

        if (shouldNavigateUp || shouldNavigateDown) {
          if (this.userPromptHistoryIndex === -1 || this.userPromptHistoryIndex === this.userPromptHistory.length) {
            this.lastUnsentUserPrompt = {
              inputText: this.promptTextInput.getTextInputValue(),
              codeAttachment: this.promptAttachment?.lastAttachmentContent ?? '',
            };
          }

          if (this.userPromptHistoryIndex === -1) {
            this.userPromptHistoryIndex = this.userPromptHistory.length;
          }

          if (e.key === KeyMap.ARROW_UP) {
            // Check if the cursor is on the first line or not
            this.userPromptHistoryIndex = Math.max(0, this.userPromptHistoryIndex - 1);
          } else if (e.key === KeyMap.ARROW_DOWN) {
            // Check if the cursor is on the last line or not
            this.userPromptHistoryIndex = Math.min(this.userPromptHistory.length, this.userPromptHistoryIndex + 1);
          }

          let codeAttachment = '';
          if (this.userPromptHistoryIndex === this.userPromptHistory.length) {
            this.promptTextInput.updateTextInputValue(this.lastUnsentUserPrompt.inputText ?? '');
            codeAttachment = this.lastUnsentUserPrompt.codeAttachment ?? '';
          } else {
            this.promptTextInput.updateTextInputValue(this.userPromptHistory[this.userPromptHistoryIndex].inputText);
            codeAttachment = this.userPromptHistory[this.userPromptHistoryIndex].codeAttachment ?? '';
          }
          codeAttachment = codeAttachment.trim();
          if (codeAttachment.length > 0) {
            // the way we mark code in our example mynah client
            if (codeAttachment.startsWith('~~~~~~~~~~') && codeAttachment.endsWith('~~~~~~~~~~')) {
              codeAttachment = codeAttachment
                .replace(/^~~~~~~~~~~/, '')
                .replace(/~~~~~~~~~~$/, '')
                .trim();
            } else if (codeAttachment.startsWith('```') && codeAttachment.endsWith('```')) {
              // the way code is marked in VScode and JetBrains extensions
              codeAttachment = codeAttachment
                .replace(/^```/, '')
                .replace(/```$/, '')
                .trim();
            }
            this.promptAttachment.updateAttachment(codeAttachment, 'code');
          } else {
            this.promptAttachment.clear();
          }
        }
      }
    } else {
      const blockedKeys = [ KeyMap.ENTER, KeyMap.ESCAPE, KeyMap.SPACE, KeyMap.TAB, KeyMap.AT, KeyMap.BACK_SLASH, KeyMap.SLASH, KeyMap.ALT ] as string[];
      if (blockedKeys.includes(e.key)) {
        // Close quick pick overlay when space is pressed
        if (e.key === KeyMap.SPACE) {
          this.quickPick?.close();
          return;
        }
        e.preventDefault();
        if (e.key === KeyMap.ESCAPE) {
          if (this.quickPickType === 'quick-action') {
            this.clearTextArea(true);
          }
          this.quickPick?.close();
        } else if (e.key === KeyMap.ENTER || e.key === KeyMap.TAB) {
          this.searchTerm = '';
          const targetDetailedListItem = this.quickPickItemsSelectorContainer?.getTargetElement();
          if (targetDetailedListItem != null) {
            const commandToSend = convertDetailedListItemToQuickActionCommand(targetDetailedListItem);
            if (this.quickPickType === 'context') {
              if (commandToSend.command !== '') {
                // Add context item to top bar if Alt-Enter is pressed on an item
                this.handleContextCommandSelection(commandToSend, e.altKey);
              } else {
                // Otherwise pass the given text by user
                const command = this.promptTextInput.getTextInputValue().substring(this.quickPickTriggerIndex, this.promptTextInput.getCursorPos());
                this.handleContextCommandSelection({ command });
              }
            } else {
              switch (e.key) {
                case KeyMap.TAB:
                  this.handleQuickActionCommandSelection(commandToSend, 'tab');
                  break;
                case KeyMap.ENTER:
                  this.handleQuickActionCommandSelection(commandToSend, 'enter');
                  break;
              }
            }
          }
        }
      } else if (navigationalKeys.includes(e.key)) {
        cancelEvent(e);
        this.quickPickItemsSelectorContainer?.changeTarget(e.key === KeyMap.ARROW_UP ? 'up' : 'down', true, true);
      } else {
        if (this.quickPick != null) {
          if (this.promptTextInput.getTextInputValue() === '') {
            this.quickPick.close();
          } else {
            if (e.key === KeyMap.ARROW_LEFT || e.key === KeyMap.ARROW_RIGHT) {
              cancelEvent(e);
            } else {
              this.filteredQuickPickItemGroups = [];
              // In case the prompt is an incomplete regex
              try {
                if (e.key === KeyMap.BACKSPACE) {
                  const isAllSelected = window.getSelection()?.toString() === this.promptTextInput.getTextInputValue();
                  if (this.searchTerm === '' || isAllSelected) {
                    this.quickPick.close();
                  } else {
                    this.searchTerm = this.searchTerm.slice(0, -1);
                  }
                } else if ((!e.ctrlKey && !e.metaKey) && e.key.length === 1) {
                  this.searchTerm += e.key.toLowerCase();
                }
                this.filteredQuickPickItemGroups = filterQuickPickItems([ ...this.quickPickItemGroups ], this.searchTerm);
              } catch (e) {}
              if (this.filteredQuickPickItemGroups.length > 0) {
                this.quickPick.toggleHidden(false);
                this.quickPick.updateContent([ this.getQuickPickItemGroups(this.filteredQuickPickItemGroups) ]);
              } else {
              // If there's no matching action, hide the command selector overlay
                this.quickPick.toggleHidden(true);
              }
            }
          }
        }
      }
    }
  };

  private readonly tabBarTitleOverlayKeyPressHandler = (e: KeyboardEvent): void => {
    if (e.key === KeyMap.ARROW_UP || e.key === KeyMap.ARROW_DOWN) {
      cancelEvent(e);
      this.quickPickItemsSelectorContainer?.changeTarget(e.key === KeyMap.ARROW_UP ? 'up' : 'down', true, true);
    } else if (e.key === KeyMap.ENTER) {
      const detailedListItem = this.quickPickItemsSelectorContainer?.getTargetElement();
      if (detailedListItem != null) {
        const quickPickCommand: QuickActionCommand = convertDetailedListItemToQuickActionCommand(detailedListItem);
        this.handleContextCommandSelection(quickPickCommand);
      }
    } else if (e.key === KeyMap.ESCAPE) {
      this.quickPick.close();
      if (Config.getInstance().config.autoFocus) {
        this.promptTextInput.focus();
      }
    }
  };

  private readonly openQuickPick = (topBarTitleClicked?: boolean): void => {
    this.topBarTitleClicked = topBarTitleClicked === true;

    this.quickPickItemsSelectorContainer = null;

    if (this.topBarTitleClicked) {
      window.addEventListener('keydown', this.tabBarTitleOverlayKeyPressHandler);
    }

    if (this.quickPickItemGroups.length > 0) {
      this.quickPick = new Overlay({
        closeOnOutsideClick: true,
        referenceElement: this.render.querySelector('.mynah-chat-prompt') as ExtendedHTMLElement,
        dimOutside: false,
        stretchWidth: true,
        verticalDirection: OverlayVerticalDirection.TO_TOP,
        horizontalDirection: OverlayHorizontalDirection.START_TO_RIGHT,
        onClose: () => {
          this.quickPickOpen = false;
          window.removeEventListener('keydown', this.tabBarTitleOverlayKeyPressHandler);
        },
        children: [
          this.getQuickPickItemGroups(this.filteredQuickPickItemGroups)
        ],
      });

      this.quickPickOpen = true;
    }
  };

  private readonly handleInputFocus = (): void => {
    // Show the character limit warning overlay if the threshold is hit
    this.updateAvailableCharactersIndicator();

    const inputValue = this.promptTextInput.getTextInputValue();
    if (inputValue.startsWith('/')) {
      const quickPickItems = MynahUITabsStore.getInstance().getTabDataStore(this.props.tabId).getValue('quickActionCommands') as QuickActionCommandGroup[];
      this.quickPickItemGroups = [ ...quickPickItems ];
      this.quickPickTriggerIndex = 1;
      const restorePreviousFilteredQuickPickItemGroups: QuickActionCommandGroup[] = [];
      this.quickPickItemGroups.forEach((quickPickGroup: QuickActionCommandGroup) => {
        const newQuickPickCommandGroup = { ...quickPickGroup };
        try {
          const searchTerm = inputValue.substring(this.quickPickTriggerIndex).match(/\S*/gi)?.[0];
          const promptRegex = new RegExp(searchTerm ?? '', 'gi');
          newQuickPickCommandGroup.commands = newQuickPickCommandGroup.commands.filter(command =>
            command.command.match(promptRegex)
          );
          if (newQuickPickCommandGroup.commands.length > 0) {
            restorePreviousFilteredQuickPickItemGroups.push(newQuickPickCommandGroup);
          }
        } catch (e) {
          // In case the prompt is an incomplete regex
        }
      });

      this.filteredQuickPickItemGroups = [ ...restorePreviousFilteredQuickPickItemGroups ];
      this.openQuickPick();
    }
  };

  private readonly getQuickPickItemGroups = (quickPickGroupList: QuickActionCommandGroup[]): ExtendedHTMLElement => {
    const detailedListItemsGroup = convertQuickActionCommandGroupsToDetailedListGroups(quickPickGroupList);
    if (this.quickPickItemsSelectorContainer == null) {
      const pinContextHint = Config.getInstance().config.texts.pinContextHint;
      this.quickPickItemsSelectorContainer = new DetailedListWrapper({
        descriptionTextDirection: 'rtl',
        detailedList: {
          list: detailedListItemsGroup,
          selectable: true,
          ...(this.topBarTitleClicked
            ? {
                filterOptions: [
                  {
                    type: 'textinput',
                    icon: MynahIcons.SEARCH,
                    id: 'search',
                    placeholder: 'Search context',
                    autoFocus: true,
                  },
                ],

              }
            : !this.promptTopBar.isHidden() && this.quickPickType === 'context' && pinContextHint !== ''
                ? {
                    header: {
                      description: pinContextHint,
                    }
                  }
                : {})
        },
        ...(this.topBarTitleClicked
          ? {
              onFilterValueChange: (filterValues) => {
                const searchTerm = filterValues?.search ?? '';
                if (searchTerm.length > 0) { this.filteredQuickPickItemGroups = filterQuickPickItems([ ...this.quickPickItemGroups ], searchTerm, this.topBarTitleClicked); } else {
                  this.filteredQuickPickItemGroups = [ ...this.quickPickItemGroups ];
                }
                const results = convertQuickActionCommandGroupsToDetailedListGroups(this.filteredQuickPickItemGroups);
                const emptyResults = results.length === 0 || (results.length === 1 && results[0].children?.length === 0);
                this.quickPickItemsSelectorContainer?.update({ list: emptyResults ? [ { groupName: 'No matches found' } ] : results });
              },
            }
          : {}),
        onGroupActionClick: (action) => {
          this.promptTextInput.deleteTextRange(this.quickPickTriggerIndex, this.promptTextInput.getCursorPos());
          MynahUIGlobalEvents.getInstance().dispatch(MynahEventNames.QUICK_COMMAND_GROUP_ACTION_CLICK, {
            tabId: this.props.tabId,
            actionId: action.id
          });
        },
        onItemSelect: (detailedListItem) => {
          const quickPickCommand: QuickActionCommand = convertDetailedListItemToQuickActionCommand(detailedListItem);
          if (this.quickPickType === 'context') {
            this.handleContextCommandSelection(quickPickCommand);
          } else {
            this.handleQuickActionCommandSelection(quickPickCommand, 'click');
          }
        },
      });
    } else {
      this.quickPickItemsSelectorContainer.update({
        list: detailedListItemsGroup
      });
    }

    const headerInfo: QuickActionCommandsHeader = MynahUITabsStore.getInstance().getTabDataStore(this.props.tabId).getValue('quickActionCommandsHeader');
    let headerComponent = new TitleDescriptionWithIcon({
      ...headerInfo,
      classNames: [ 'mynah-chat-prompt-quick-picks-header', `status-${headerInfo.status ?? 'default'}` ]
    }).render;

    // const subscriptionId =
    MynahUITabsStore.getInstance().addListenerToDataStore(this.props.tabId, 'quickActionCommandsHeader', (newHeader: QuickActionCommandsHeader) => {
      const newHeaderComponent = new TitleDescriptionWithIcon({
        ...newHeader,
        classNames: [ 'mynah-chat-prompt-quick-picks-header', `status-${newHeader.status ?? 'default'}` ]
      }).render;

      headerComponent.replaceWith(newHeaderComponent);
      headerComponent = newHeaderComponent;
    });

    // Only show header if it has meaningful content
    const hasHeaderContent = headerInfo != null && (
      (headerInfo.title != null && headerInfo.title.trim() !== '') ||
      (headerInfo.description != null && headerInfo.description.trim() !== '') ||
      headerInfo.icon != null
    );

    return DomBuilder.getInstance().build({
      type: 'div',
      classNames: [ 'mynah-chat-prompt-quick-picks-overlay-wrapper' ],
      children: [
        ...(this.quickPickType === 'quick-action' && hasHeaderContent
          ? [ headerComponent ]
          : []),
        this.quickPickItemsSelectorContainer.render
      ]
    });
  };

  private readonly handleQuickActionCommandSelection = (
    dirtyQuickActionCommand: QuickActionCommand,
    method: 'enter' | 'tab' | 'space' | 'click'): void => {
    const quickActionCommand = {
      ...dirtyQuickActionCommand,
      command: dirtyQuickActionCommand.command.replace(this.markerRemovalRegex, '')
    };

    this.selectedCommand = quickActionCommand.command;
    this.promptTextInput.updateTextInputValue('');
    if (quickActionCommand.placeholder !== undefined) {
      this.promptTextInputCommand.setCommand(this.selectedCommand);
      this.promptTextInput.updateTextInputPlaceholder(quickActionCommand.placeholder);
      if (Config.getInstance().config.autoFocus) {
        this.promptTextInput.focus();
      }
    } else if (method === 'enter' || method === 'click') {
      this.sendPrompt();
    } else {
      this.promptTextInputCommand.setCommand(this.selectedCommand);
      this.promptTextInput.updateTextInputPlaceholder(Config.getInstance().config.texts.commandConfirmation);
      this.promptTextInput.updateTextInputMaxLength(0);
      this.render.addClass('awaits-confirmation');
    }
    this.quickPick.close();
  };

  private readonly handleContextCommandSelection = (dirtyContextCommand: QuickActionCommand, topBarHotKey?: boolean): void => {
    const contextCommand: QuickActionCommand = {
      ...dirtyContextCommand,
      command: dirtyContextCommand.command.replace(this.markerRemovalRegex, '')
    };
    // Check if the selected command has children
    if (contextCommand.children?.[0] != null) {
      // If user types '@fi', and then selects a command with children (ex: file command), remove 'fi' from prompt
      if (!this.topBarTitleClicked) { this.promptTextInput.deleteTextRange(this.quickPickTriggerIndex + 1, this.promptTextInput.getCursorPos()); }
      this.quickPickItemGroups = [ ...contextCommand.children ];
      this.quickPick.updateContent([
        this.getQuickPickItemGroups(contextCommand.children)
      ]);
    } else {
      if (this.quickPickTriggerRange != null) {
        // Restore cursor position so element is inserted in correct position
        this.promptTextInput.restoreRange(this.quickPickTriggerRange);
      }
      this.quickPick.close();
      MynahUIGlobalEvents.getInstance().dispatch(MynahEventNames.CONTEXT_SELECTED, {
        contextItem: contextCommand,
        tabId: this.props.tabId,
        promptInputCallback: (insert: boolean) => {
          if (insert) {
            // Add command to top bar if top bar is visible, and either top bar title was clicked or topBarHotKey used
            if (!this.promptTopBar.isHidden() && (this.topBarTitleClicked || topBarHotKey === true)) {
              this.promptTopBar.addContextPill(contextCommand);
              // If user types `@foo` to add context but used topBarHotKey, remove `@foo` from prompt
              if (topBarHotKey === true && !this.topBarTitleClicked) { this.promptTextInput.deleteTextRange(this.quickPickTriggerIndex, this.promptTextInput.getCursorPos()); }
            } else {
              this.promptTextInput.insertContextItem({
                ...contextCommand,
              }, this.quickPickTriggerIndex, this.promptTopBar.isHidden());
            }
          } else {
            this.promptTextInput.deleteTextRange(this.quickPickTriggerIndex, this.promptTextInput.getCursorPos());
          }
        }
      });
    }
  };

  private readonly sendPrompt = (): void => {
    const quickPickItems = MynahUITabsStore.getInstance().getTabDataStore(this.props.tabId).getValue('quickActionCommands') as QuickActionCommandGroup[];
    const currentInputValue = this.promptTextInput.getTextInputValue();
    if (currentInputValue !== '' || this.selectedCommand.trim() !== '') {
      let selectedCommand = this.selectedCommand;

      // Catching cases where user could send a prompt with quick action command but the command is not be selected correctly
      if (selectedCommand === '') {
        for (const quickPickItem of quickPickItems) {
          if (selectedCommand !== '') break;
          const matchedCommand = quickPickItem.commands.find((item) => item.disabled === false && currentInputValue.startsWith(item.command));
          if (matchedCommand !== undefined) {
            selectedCommand = matchedCommand.command;
          }
        }
      }

      const attachmentContent: string | undefined = this.promptAttachment?.lastAttachmentContent;

      // Trim prompt text with command selectedCommand exists
      const promptText = this.selectedCommand === '' && selectedCommand !== ''
        ? currentInputValue.replace(selectedCommand, '') + (attachmentContent ?? '')
        : currentInputValue + (attachmentContent ?? '');
      const context: QuickActionCommand[] = this.promptTextInput.getUsedContext();

      let escapedPrompt = escapeHTML(promptText);
      context?.forEach(cmd => {
        if (cmd.command !== '') {
          // Escape special regex characters in the command
          const escapedCmd = cmd.command.replace(/[.*+?^${}()|[\]\\]/g, '\\$&');
          // Replace all occurrences of @command with **@command**
          escapedPrompt = escapedPrompt.replace(
            new RegExp(`@${escapedCmd}`, 'g'),
            ` **@${cmd.command}**`
          );
        }
      });

      const promptData: {tabId: string; prompt: ChatPrompt} = {
        tabId: this.props.tabId,
        prompt: {
          prompt: promptText,
          escapedPrompt,
          context,
          options: this.promptOptions.getOptionValues() ?? {},
          ...(selectedCommand !== '' ? { command: selectedCommand } : {}),
        }
      };
      this.clearTextArea();

      if (currentInputValue !== '') {
        this.userPromptHistory.push({
          inputText: currentInputValue,
          codeAttachment: attachmentContent ?? '',
        });
      }

      this.lastUnsentUserPrompt = {
        inputText: '',
        codeAttachment: '',
      };

      this.userPromptHistoryIndex = -1;
      MynahUIGlobalEvents.getInstance().dispatch(MynahEventNames.CHAT_PROMPT, promptData);
    }
  };

  private readonly getPromptInputTextLabel = (promptInputLabel?: string): ExtendedHTMLElement => DomBuilder.getInstance().build({
    type: 'div',
    testId: testIds.prompt.label,
    classNames: [ 'mynah-chat-prompt-input-label' ],
    children: promptInputLabel != null && promptInputLabel.trim() !== ''
      ? [
          new CardBody({
            body: promptInputLabel
          }).render
        ]
      : []
  });

  public readonly clearTextArea = (keepAttachment?: boolean): void => {
    MynahUITabsStore.getInstance().getTabDataStore(this.props.tabId).updateStore({
      promptInputText: '',
    });
    this.selectedCommand = '';
    this.promptTextInput.clear();
    this.promptTextInput.updateTextInputMaxLength(MAX_USER_INPUT());
    this.promptTextInputCommand.setCommand('');
    if (keepAttachment !== true) {
      this.attachmentWrapper.clear();
      this.promptAttachment.clear();
    }
    this.updateAvailableCharactersIndicator();
  };

  public readonly getPromptInputText = (): string => {
    return this.promptTextInput.getTextInputValue();
  };

  public readonly getCursorPosition = (): number => {
    return this.promptTextInput.getCursorPos();
  };

  public readonly addAttachment = (attachmentContent: string, type?: PromptAttachmentType): void => {
    MynahUIGlobalEvents.getInstance().dispatch(MynahEventNames.ADD_ATTACHMENT, {
      textToAdd: attachmentContent,
      tabId: this.props.tabId,
      type
    });
  };

  public readonly openTopBarButtonItemOverlay = (data: TopBarButtonOverlayProps): void => {
    this.promptTopBar.topBarButton.showOverlay(data);
  };

  public readonly updateTopBarButtonItemOverlay = (data: DetailedList): void => {
    this.promptTopBar.topBarButton.onTopBarButtonOverlayChanged(data);
  };

  public readonly closeTopBarButtonItemOverlay = (): void => {
    this.promptTopBar.topBarButton.closeOverlay();
  };

  public readonly destroy = (): void => {
    this.promptTextInput.destroy();
  };

  public readonly getCurrentTriggerSource = (): 'top-bar' | 'prompt-input' => {
    return this.topBarTitleClicked ? 'top-bar' : 'prompt-input';
  };
}
