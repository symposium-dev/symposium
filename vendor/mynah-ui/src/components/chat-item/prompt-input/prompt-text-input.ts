import { Config } from '../../../helper/config';
import { DomBuilder, ExtendedHTMLElement } from '../../../helper/dom';
import { MynahUIGlobalEvents } from '../../../helper/events';
import { MynahUITabsStore } from '../../../helper/tabs-store';
import { MynahEventNames, QuickActionCommand, QuickActionCommandGroup } from '../../../static';
import { MAX_USER_INPUT } from '../chat-prompt-input';
import { Overlay, OverlayHorizontalDirection, OverlayVerticalDirection } from '../../overlay';
import { Card } from '../../card/card';
import { CardBody } from '../../card/card-body';
import testIds from '../../../helper/test-ids';
import { generateUID } from '../../../main';
import { Icon, MynahIcons } from '../../icon';

const PREVIEW_DELAY = 500;
const IMAGE_CONTEXT_SELECT_KEYWORD = '@image:';
export interface PromptTextInputProps {
  tabId: string;
  initMaxLength: number;
  children?: ExtendedHTMLElement[];
  onKeydown: (e: KeyboardEvent) => void;
  onInput?: (e: KeyboardEvent) => void;
  onFocus?: () => void;
  onBlur?: () => void;
}

export class PromptTextInput {
  render: ExtendedHTMLElement;
  promptTextInputMaxLength: number;
  private lastCursorIndex: number = 0;
  private readonly props: PromptTextInputProps;
  private readonly promptTextInput: ExtendedHTMLElement;
  private promptInputOverlay: Overlay | null = null;
  private keydownSupport: boolean = true;
  private readonly selectedContext: Record<string, QuickActionCommand> = {};
  private contextTooltip: Overlay | null;
  private contextTooltipTimeout: ReturnType<typeof setTimeout>;
  private mutationObserver: MutationObserver | null = null;

  constructor (props: PromptTextInputProps) {
    this.props = props;
    this.promptTextInputMaxLength = props.initMaxLength;

    const initialDisabledState = MynahUITabsStore.getInstance().getTabDataStore(this.props.tabId).getValue('promptInputDisabledState') as boolean;

    this.promptTextInput = DomBuilder.getInstance().build({
      type: 'div',
      testId: testIds.prompt.input,
      classNames: [ 'mynah-chat-prompt-input', 'empty' ],
      innerHTML: '',
      attributes: {
        contenteditable: 'plaintext-only',
        ...(initialDisabledState ? { disabled: 'disabled' } : {}),
        tabindex: '0',
        rows: '1',
        maxlength: MAX_USER_INPUT().toString(),
        type: 'text',
        placeholder: MynahUITabsStore.getInstance().getTabDataStore(this.props.tabId).getValue('promptInputPlaceholder'),
        ...(Config.getInstance().config.autoFocus ? { autofocus: 'autofocus' } : {})
      },
      events: {
        keypress: (e: KeyboardEvent) => {
          if (!this.keydownSupport) {
            this.props.onKeydown(e);
          }
        },
        keydown: (e: KeyboardEvent) => {
          if (e.key !== '') {
            this.keydownSupport = true;
            this.props.onKeydown(e);
          } else {
            this.keydownSupport = false;
          }
          this.hideContextTooltip();
        },
        keyup: (e: KeyboardEvent) => {
          this.lastCursorIndex = this.updateCursorPos();

          // Check if image command exists in context commands to make the feature consistent
          const contextCommands = MynahUITabsStore.getInstance().getTabDataStore(this.props.tabId).getValue('contextCommands') as QuickActionCommandGroup[] | undefined;
          const hasImageCommand = contextCommands?.some(group =>
            group.commands.some(cmd => cmd.command.toLowerCase() === 'image')
          );

          if (hasImageCommand ?? false) {
            const text = this.promptTextInput.textContent ?? '';
            if (text.includes(IMAGE_CONTEXT_SELECT_KEYWORD)) {
              // Dispatch event to open file system
              MynahUIGlobalEvents.getInstance().dispatch(MynahEventNames.OPEN_FILE_SYSTEM, {
                tabId: this.props.tabId,
                type: 'image',
                insertPosition: this.lastCursorIndex - IMAGE_CONTEXT_SELECT_KEYWORD.length
              });

              // Remove the trigger text
              const selection = window.getSelection();
              if ((selection?.rangeCount) != null) {
                const range = selection.getRangeAt(0);
                const textNodes = Array.from(this.promptTextInput.childNodes).filter((node): node is Text => node.nodeType === Node.TEXT_NODE);

                // Find the node containing "@image:"
                for (const node of textNodes) {
                  const nodeText = node.textContent ?? '';
                  const imageTagIndex = nodeText.indexOf(IMAGE_CONTEXT_SELECT_KEYWORD);

                  if (imageTagIndex !== -1) {
                    // Create a range that selects "@image:"
                    range.setStart(node, imageTagIndex);
                    range.setEnd(node, imageTagIndex + IMAGE_CONTEXT_SELECT_KEYWORD.length);
                    range.deleteContents();
                    break;
                  }
                }
              }
            }
          }
        },
        input: (e: KeyboardEvent) => {
          if (this.props.onInput !== undefined) {
            this.props.onInput(e);
          }
          this.removeContextPlaceholderOverlay();
          this.checkIsEmpty();
        },
        focus: () => {
          if (typeof this.props.onFocus !== 'undefined') {
            this.props.onFocus();
          }
          this.lastCursorIndex = this.updateCursorPos();
        },
        blur: () => {
          if (typeof this.props.onBlur !== 'undefined') {
            this.props.onBlur();
          }
        },
        paste: (e: ClipboardEvent): void => {
          // Prevent the default paste behavior
          e.preventDefault();

          // Get plain text from clipboard
          const text = e.clipboardData?.getData('text/plain');
          if (text != null) {
            // Insert text at cursor position
            const selection = window.getSelection();
            if ((selection?.rangeCount) != null) {
              const range = selection.getRangeAt(0);
              range.deleteContents();
              range.insertNode(document.createTextNode(text));

              // Move cursor to end of inserted text
              range.collapse(false);
              selection.removeAllRanges();
              selection.addRange(range);
            }

            // Check if input is empty and trigger input event
            this.checkIsEmpty();
            if (this.props.onInput != null) {
              this.props.onInput(new KeyboardEvent('input'));
            }
          }
        },
      },
    });

    this.render = DomBuilder.getInstance().build({
      type: 'div',
      testId: testIds.prompt.inputWrapper,
      classNames: [ 'mynah-chat-prompt-input-inner-wrapper', 'no-text' ],
      children: [
        ...(this.props.children ?? []),
        this.promptTextInput,
      ]
    });

    // Set up MutationObserver to detect context span removals
    this.setupContextRemovalObserver();

    MynahUITabsStore.getInstance().getTabDataStore(this.props.tabId).subscribe('promptInputDisabledState', (isDisabled: boolean) => {
      if (isDisabled) {
        this.promptTextInput.setAttribute('disabled', 'disabled');
        this.promptTextInput.setAttribute('contenteditable', 'false');
        this.promptTextInput.blur();
      } else {
        // Enable the input field and focus on it
        this.promptTextInput.removeAttribute('disabled');
        this.promptTextInput.setAttribute('contenteditable', 'plaintext-only');
        if (Config.getInstance().config.autoFocus && document.hasFocus()) {
          this.promptTextInput.focus();
        }
      }
    });

    MynahUITabsStore.getInstance().getTabDataStore(this.props.tabId).subscribe('promptInputPlaceholder', (placeholderText: string) => {
      if (placeholderText !== undefined) {
        this.promptTextInput.update({
          attributes: {
            placeholder: placeholderText
          }
        });
      }
    });

    MynahUIGlobalEvents.getInstance().addListener(MynahEventNames.ADD_CUSTOM_CONTEXT, (data: { tabId: string; contextCommands: QuickActionCommand[]; insertPosition?: number}) => {
      if (data.tabId === this.props.tabId) {
        let insertPos = data.insertPosition ?? this.lastCursorIndex;
        data.contextCommands.forEach((command) => {
          this.insertContextItem(command, insertPos);
          insertPos = this.getCursorPos();
        });
        this.focus();
      }
    });

    MynahUIGlobalEvents.getInstance().addListener(MynahEventNames.TAB_FOCUS, (data) => {
      if (data.tabId === this.props.tabId) {
        this.promptTextInput.focus();
      }
    });

    this.clear();
  }

  private readonly setupContextRemovalObserver = (): void => {
    if (MutationObserver != null) {
      this.mutationObserver = new MutationObserver((mutations) => {
        let contextRemoved = false;
        const removedContextIds: string[] = [];

        mutations.forEach((mutation) => {
          if (mutation.type === 'childList') {
            mutation.removedNodes.forEach((node) => {
              if (node.nodeType === Node.ELEMENT_NODE) {
                const element = node as Element;
                if (element.classList.contains('context')) {
                  const contextId = element.getAttribute('context-tmp-id');
                  if (contextId != null && contextId !== '') {
                    removedContextIds.push(contextId);
                    contextRemoved = true;
                  }
                }
                // Also check for context spans within removed nodes
                const contextSpans = element.querySelectorAll('.context');
                contextSpans.forEach((span) => {
                  const contextId = span.getAttribute('context-tmp-id');
                  if (contextId != null && contextId !== '') {
                    removedContextIds.push(contextId);
                    contextRemoved = true;
                  }
                });
              }
            });
          }
        });

        if (contextRemoved) {
          this.handleContextRemoval(removedContextIds);
        }
      });

      this.mutationObserver.observe(this.promptTextInput, {
        childList: true,
        subtree: true
      });
    }
  };

  private readonly handleContextRemoval = (removedContextIds: string[]): void => {
    const currentCustomContext = MynahUITabsStore.getInstance().getTabDataStore(this.props.tabId).getValue('customContextCommand') as QuickActionCommand[] ?? [];
    const removedContexts: QuickActionCommand[] = [];

    // Find the removed contexts from our selectedContext map
    removedContextIds.forEach((contextId) => {
      const removedContext = this.selectedContext[contextId];
      if (removedContext != null) {
        removedContexts.push(removedContext);
      }
    });

    // Clean up the selectedContext map by creating a new object without the removed keys
    const updatedSelectedContext = Object.fromEntries(
      Object.entries(this.selectedContext).filter(([ key ]) => !removedContextIds.includes(key))
    );
    Object.assign(this.selectedContext, updatedSelectedContext);

    // Remove the contexts from the data store
    if (removedContexts.length > 0) {
      const updatedCustomContext = currentCustomContext.filter((context) => {
        return !removedContexts.some((removed) =>
          removed.command === context.command &&
          removed.icon === context.icon &&
          removed.description === context.description
        );
      });

      MynahUITabsStore.getInstance().getTabDataStore(this.props.tabId).updateStore({
        customContextCommand: updatedCustomContext
      });
    }
  };

  public readonly restoreRange = (range: Range): void => {
    const selection = window.getSelection();
    if (selection != null) {
      selection.removeAllRanges();
      selection.addRange(range);
      this.updateCursorPos();
    }
  };

  private readonly updateCursorPos = (): number => {
    const selection = window.getSelection();
    if ((selection == null) || (selection.rangeCount === 0)) return 0;

    const range = selection.getRangeAt(0);
    const container = this.promptTextInput;

    // If the selection is not within our container, return 0
    if (!container.contains(range.commonAncestorContainer)) return 0;

    // Get the range from start of container to cursor position
    const preCaretRange = range.cloneRange();
    preCaretRange.selectNodeContents(container);
    preCaretRange.setEnd(range.endContainer, range.endOffset);

    return preCaretRange.toString().length;
  };

  private readonly updateSelectionAndCursor = (range: Range, maintainCursor: boolean = false, selection?: Selection | null): void => {
    const sel = selection ?? window.getSelection();
    if (!maintainCursor && sel != null) {
      sel.removeAllRanges();
      sel.addRange(range);
      this.lastCursorIndex = this.updateCursorPos();
    }
  };

  private readonly insertSpaceNodeAndUpdateCursor = (element: HTMLElement | Text, maintainCursor: boolean = false, selection?: Selection | null): void => {
    const spaceNode = document.createTextNode('\u00A0');
    element.parentNode?.insertBefore(spaceNode, element.nextSibling);
    if (!maintainCursor && selection != null) {
      const endRange = document.createRange();
      endRange.setStartAfter(spaceNode);
      endRange.collapse(true);
      this.updateSelectionAndCursor(endRange, maintainCursor, selection);
    }
  };

  private readonly checkIsEmpty = (): void => {
    if (this.promptTextInput.textContent === '' && this.promptTextInput.querySelectorAll('span.context').length === 0) {
      this.promptTextInput.addClass('empty');
      this.render.addClass('no-text');
    } else {
      this.promptTextInput.removeClass('empty');
      this.render.removeClass('no-text');
    }
  };

  private readonly removeContextPlaceholderOverlay = (): void => {
    this.promptInputOverlay?.close();
    this.promptInputOverlay?.render.remove();
    this.promptInputOverlay = null;
  };

  private readonly insertElementToGivenPosition = (
    element: HTMLElement | Text,
    position: number,
    endPosition?: number,
    maintainCursor: boolean = false
  ): void => {
    const selection = window.getSelection();
    // Clamp position to total content length
    let totalLength = 0;
    for (const node of this.promptTextInput.childNodes) {
      totalLength += node.textContent?.length ?? 0;
    }
    const safePosition = Math.min(position, totalLength);

    if (this.promptTextInput.childNodes.length === 0) {
      this.promptTextInput.insertChild('beforeend', element as HTMLElement);
      this.insertSpaceNodeAndUpdateCursor(element, maintainCursor, selection);
      return;
    }

    // If the only child is a <br>, treat as empty and insert at end
    // The <br> is inserted into contenteditable html element if left empty,
    // this happens when delete prompt input by selecting all and clicking delete
    if (
      this.promptTextInput.childNodes.length === 1 &&
      this.promptTextInput.firstChild?.nodeName === 'BR'
    ) {
      // Remove the br element and insert our element
      this.promptTextInput.firstChild.remove();
      this.promptTextInput.insertChild('beforeend', element as HTMLElement);
      this.insertSpaceNodeAndUpdateCursor(element, maintainCursor, selection);
      return;
    }

    if (selection == null ||
      (selection.focusNode?.isSameNode(this.promptTextInput) === false &&
      selection.focusNode?.parentElement?.isSameNode(this.promptTextInput) === false)) {
      this.promptTextInput.insertChild('beforeend', element as HTMLElement);
      return;
    }

    // Store original cursor position if we need to maintain it
    const originalRange = maintainCursor ? selection.getRangeAt(0).cloneRange() : null;

    const range = document.createRange();
    let currentPos = 0;
    let foundInsertionPoint = false;

    // Find the correct text node and offset
    for (const node of this.promptTextInput.childNodes) {
      const length = node.textContent?.length ?? 0;

      // Only try to set range on text nodes
      if (currentPos + length >= safePosition && (node.nodeType === Node.TEXT_NODE)) {
        const offset = Math.min(safePosition - currentPos, length);
        // Defensive: only setStart if offset is valid
        if (offset >= 0 && offset <= length) {
          range.setStart(node, offset);

          if (endPosition != null) {
            let endNode = node;
            let endOffset = Math.min(endPosition - currentPos, length);

            if (endPosition > currentPos + length) {
              let endPos = currentPos + length;
              for (let i = Array.from(this.promptTextInput.childNodes).indexOf(node) + 1;
                i < this.promptTextInput.childNodes.length;
                i++) {
                const nextNode = this.promptTextInput.childNodes[i];
                const nextLength = nextNode.textContent?.length ?? 0;

                if (endPos + nextLength >= endPosition) {
                  endNode = nextNode;
                  endOffset = endPosition - endPos;
                  break;
                }
                endPos += nextLength;
              }
            }

            range.setEnd(endNode, endOffset);
            range.deleteContents();
          }

          range.insertNode(element);

          if (endPosition != null) {
            const spaceNode = document.createTextNode('\u00A0');
            range.setStartAfter(element);
            range.insertNode(spaceNode);
            range.setStartAfter(spaceNode);
            element = spaceNode;
          } else {
            range.setStartAfter(element);
          }
          foundInsertionPoint = true;
          break;
        }
      }
      currentPos += length;
    }

    // Fallback: if nothing was inserted, insert at the end
    if (!foundInsertionPoint) {
      this.promptTextInput.insertChild('beforeend', element as HTMLElement);
      this.insertSpaceNodeAndUpdateCursor(element, maintainCursor, selection);
      return;
    }

    if (!maintainCursor) {
      // Only modify cursor position if maintainCursor is false
      range.collapse(true);
      this.updateSelectionAndCursor(range, maintainCursor, selection);
    } else if (originalRange != null) {
      // Restore original cursor position
      selection.removeAllRanges();
      selection.addRange(originalRange);
    }
  };

  private readonly moveCursorToEnd = (): void => {
    const range = document.createRange();
    range.selectNodeContents(this.promptTextInput);
    range.collapse(false);
    const selection = window.getSelection();
    if (selection != null) {
      selection.removeAllRanges();
      selection.addRange(range);
    }
  };

  private readonly showContextTooltip = (e: MouseEvent, contextItem: QuickActionCommand): void => {
    clearTimeout(this.contextTooltipTimeout);
    this.contextTooltipTimeout = setTimeout(() => {
      const elm: HTMLElement = e.target as HTMLElement;
      this.contextTooltip = new Overlay({
        background: true,
        closeOnOutsideClick: false,
        referenceElement: elm,
        dimOutside: false,
        removeOtherOverlays: true,
        verticalDirection: OverlayVerticalDirection.TO_TOP,
        horizontalDirection: OverlayHorizontalDirection.START_TO_RIGHT,
        children: [
          DomBuilder.getInstance().build({
            type: 'div',
            testId: testIds.prompt.contextTooltip,
            classNames: [ 'mynah-chat-prompt-context-tooltip' ],
            children: [
              ...(contextItem.icon !== undefined
                ? [
                    new Icon({
                      icon: contextItem.icon
                    }).render
                  ]
                : []),
              {
                type: 'div',
                classNames: [ 'mynah-chat-prompt-context-tooltip-container' ],
                children: [
                  {
                    type: 'div',
                    classNames: [ 'mynah-chat-prompt-context-tooltip-name' ],
                    children: [ contextItem.command ]
                  },
                  ...(contextItem.description !== undefined
                    ? [ {
                        type: 'div',
                        classNames: [ 'mynah-chat-prompt-context-tooltip-description' ],
                        children: [ contextItem.description ]
                      } ]
                    : [])
                ]
              }
            ]
          })
        ],
      });
    }, PREVIEW_DELAY);
  };

  private readonly hideContextTooltip = (): void => {
    if (this.contextTooltipTimeout !== null) {
      clearTimeout(this.contextTooltipTimeout);
    }
    if (this.contextTooltip != null) {
      this.contextTooltip.close();
      this.contextTooltip = null;
    }
  };

  public readonly insertContextItem = (contextItem: QuickActionCommand, position: number, topBarHidden?: boolean): void => {
    const temporaryId = generateUID();
    this.selectedContext[temporaryId] = contextItem;
    const contextSpanElement = DomBuilder.getInstance().build({
      type: 'span',
      children: [
        ...(topBarHidden !== true ? [ new Icon({ icon: MynahIcons.PIN, classNames: [ 'hover-icon' ] }).render ] : []),
        new Icon({ icon: contextItem.icon ?? MynahIcons.AT }).render,
        { type: 'span', classNames: [ 'at-char' ], innerHTML: '@' },
        `${contextItem.command.replace(/^@?(.*)$/, '$1')}`
      ],
      classNames: [ 'context', topBarHidden === true ? 'no-hover' : '' ],
      attributes: {
        'context-tmp-id': temporaryId,
        contenteditable: 'false'
      },
      events: {
        mouseenter: (e) => {
          this.showContextTooltip(e, contextItem);
        },
        mouseleave: () => {
          this.hideContextTooltip();
        },
        click: () => {
          this.hideContextTooltip();
          MynahUIGlobalEvents.getInstance().dispatch(MynahEventNames.CONTEXT_PINNED, { tabId: this.props.tabId, contextItem });
        }
      }
    });
    this.insertElementToGivenPosition(contextSpanElement, position, this.getCursorPos());

    if (contextItem.placeholder != null) {
      this.promptInputOverlay = new Overlay({
        background: true,
        closeOnOutsideClick: true,
        referenceElement: contextSpanElement ?? this.render,
        dimOutside: false,
        removeOtherOverlays: true,
        verticalDirection: OverlayVerticalDirection.TO_TOP,
        horizontalDirection: OverlayHorizontalDirection.START_TO_RIGHT,
        children: [
          new Card({
            border: false,
            children: [
              new CardBody({
                body: contextItem.placeholder
              }).render
            ]
          }).render
        ],
      });
    }

    this.checkIsEmpty();
  };

  public readonly getCursorPos = (): number => this.lastCursorIndex;

  public readonly clear = (): void => {
    this.promptTextInput.innerHTML = '';
    const defaultPlaceholder = MynahUITabsStore.getInstance().getTabDataStore(this.props.tabId).getValue('promptInputPlaceholder');
    this.updateTextInputPlaceholder(defaultPlaceholder);
    this.removeContextPlaceholderOverlay();
    this.checkIsEmpty();
  };

  public readonly focus = (): void => {
    if (Config.getInstance().config.autoFocus) {
      this.promptTextInput.focus();
    }
    this.moveCursorToEnd();
  };

  public readonly blur = (): void => {
    this.promptTextInput.blur();
    this.checkIsEmpty();
  };

  public readonly getTextInputValue = (withInputLineBreaks?: boolean): string => {
    if (withInputLineBreaks === true) {
      return (this.promptTextInput.innerText ?? '').trim();
    }
    return (this.promptTextInput.textContent ?? '').trim();
  };

  public readonly updateTextInputValue = (value: string): void => {
    this.promptTextInput.innerText = value;
    this.checkIsEmpty();
  };

  public readonly insertEndSpace = (): void => {
    this.promptTextInput.insertAdjacentHTML('beforeend', '&nbsp;');
  };

  public readonly updateTextInputMaxLength = (maxLength: number): void => {
    this.promptTextInputMaxLength = maxLength;
    this.promptTextInput.update({
      attributes: {
        maxlength: maxLength.toString(),
      }
    });
  };

  public readonly updateTextInputPlaceholder = (text: string): void => {
    this.promptTextInput.update({
      attributes: {
        placeholder: text,
      }
    });
  };

  public readonly deleteTextRange = (position: number, endPosition: number): void => {
    const selection = window.getSelection();
    if (selection == null) return;

    const range = document.createRange();
    let currentPos = 0;
    let startNode = null;
    let startOffset = 0;
    let endNode = null;
    let endOffset = 0;

    // Find start and end positions
    for (const node of this.promptTextInput.childNodes) {
      const length = node.textContent?.length ?? 0;

      // Find start position
      if ((startNode == null) && currentPos + length >= position) {
        startNode = node;
        startOffset = position - currentPos;
      }

      // Find end position
      if (currentPos + length >= endPosition) {
        endNode = node;
        endOffset = endPosition - currentPos;
        break;
      }

      currentPos += length;
    }

    // If we found both positions, delete the range
    if ((startNode != null) && (endNode != null)) {
      range.setStart(startNode, startOffset);
      range.setEnd(endNode, endOffset);
      range.deleteContents();
    }

    this.checkIsEmpty();
  };

  /**
   * Returns the cursorLine, totalLines and if the cursor is at the beginning or end of the whole text
   * @returns {cursorLine: number, totalLines: number, isAtTheBeginning: boolean, isAtTheEnd: boolean}
   */
  public readonly getCursorPosition = (): { cursorLine: number; totalLines: number; isAtTheBeginning: boolean; isAtTheEnd: boolean } => {
    const lineHeight = parseFloat(window.getComputedStyle(this.promptTextInput, null).getPropertyValue('line-height'));
    let isAtTheBeginning = false;
    let isAtTheEnd = false;
    let cursorLine = -1;
    const cursorElm = DomBuilder.getInstance().build({
      type: 'span',
      classNames: [ 'cursor' ]
    }) as HTMLSpanElement;
    this.insertElementToGivenPosition(cursorElm, this.getCursorPos(), undefined, true);
    cursorLine = Math.floor((cursorElm.offsetTop + (cursorElm.offsetHeight)) / lineHeight) ?? 0;
    if (cursorLine <= 1 && (cursorElm?.offsetLeft ?? 0) === 0) {
      isAtTheBeginning = true;
    }

    const eolElm = DomBuilder.getInstance().build({
      type: 'span',
      classNames: [ 'eol' ]
    }) as HTMLSpanElement;
    this.promptTextInput.insertChild('beforeend', eolElm);
    const totalLines = Math.floor((eolElm.offsetTop + (eolElm.offsetHeight)) / lineHeight) ?? 0;
    if (cursorElm.offsetLeft === eolElm.offsetLeft && cursorElm.offsetTop === eolElm.offsetTop) {
      isAtTheEnd = true;
    }

    cursorElm.remove();
    eolElm.remove();

    return {
      cursorLine,
      totalLines,
      isAtTheBeginning,
      isAtTheEnd,
    };
  };

  public readonly getUsedContext = (): QuickActionCommand[] => {
    return Array.from(this.promptTextInput.querySelectorAll('span.context')).map((context) => {
      return this.selectedContext[context.getAttribute('context-tmp-id') ?? ''] ?? {};
    });
  };

  public readonly destroy = (): void => {
    if (this.mutationObserver != null) {
      this.mutationObserver.disconnect();
      this.mutationObserver = null;
    }
  };
}
