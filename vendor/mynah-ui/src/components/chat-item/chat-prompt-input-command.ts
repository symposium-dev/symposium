/*!
 * Copyright 2022 Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

import { DomBuilder, ExtendedHTMLElement } from '../../helper/dom';
import testIds from '../../helper/test-ids';

export interface ChatPromptInputCommandProps {
  command: string;
  onRemoveClick: () => void;
}
export class ChatPromptInputCommand {
  render: ExtendedHTMLElement;
  private readonly props: ChatPromptInputCommandProps;
  private readonly promptTextInputCommand: ExtendedHTMLElement;
  constructor (props: ChatPromptInputCommandProps) {
    this.props = props;
    this.promptTextInputCommand = DomBuilder.getInstance().build({
      type: 'span',
      classNames: [ 'mynah-chat-prompt-input-command-text' ],
      events: {
        click: this.props.onRemoveClick
      }
    });
    this.render = DomBuilder.getInstance().build({
      type: 'span',
      testId: testIds.prompt.selectedCommand,
      classNames: [ 'mynah-chat-prompt-input-command-wrapper', this.props.command === '' ? 'hidden' : '' ],
      children: [
        this.promptTextInputCommand,
      ]
    });
  }

  setCommand = (command: string): void => {
    if (command.trim() === '') {
      this.render.addClass('hidden');
    } else {
      this.render.removeClass('hidden');
    }
    this.promptTextInputCommand.innerText = command;
  };
}
