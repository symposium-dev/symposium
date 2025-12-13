/*!
 * Copyright 2022 Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

import { Config } from '../../helper/config';
import { DomBuilder, ExtendedHTMLElement } from '../../helper/dom';
import testIds from '../../helper/test-ids';
import { ReferenceTrackerInformation } from '../../static';
import { CardBody } from '../card/card-body';
import { CollapsibleContent } from '../collapsible-content';

export interface ChatItemTreeViewLicenseProps {
  referenceSuggestionLabel: string;
  references: ReferenceTrackerInformation[];
}

export class ChatItemTreeViewLicense {
  render: ExtendedHTMLElement;

  constructor (props: ChatItemTreeViewLicenseProps) {
    // If no references are found then just return an empty div
    if (props.references.length === 0) {
      this.render = DomBuilder.getInstance().build({
        type: 'span',
        classNames: [ 'empty' ]
      });
      return;
    }

    this.render = new CollapsibleContent({
      title: Config.getInstance().config.texts.codeSuggestionWithReferenceTitle,
      testId: testIds.chatItem.fileTree.license,
      classNames: [ 'mynah-chat-item-tree-view-license' ],
      children: [ this.buildDropdownChildren(props.references) ]
    }).render;
  }

  private readonly buildDropdownChildren = (references: ReferenceTrackerInformation[]): ExtendedHTMLElement => DomBuilder.getInstance().build({
    type: 'ul',
    classNames: [ 'mynah-chat-item-tree-view-license-container' ],
    children: references.map(ref => ({
      type: 'li',
      children: [
        new CardBody({
          body: ref.information
        }).render
      ]
    })),
  });
}
