/*!
 * Copyright 2022 Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

import { DomBuilder, ExtendedHTMLElement } from '../../helper/dom';
import testIds from '../../helper/test-ids';

export interface FeedbackFormCommentProps {
  onChange?: (comment: string) => void;
  initComment?: string;
}
export class FeedbackFormComment {
  render: ExtendedHTMLElement;

  constructor (props: FeedbackFormCommentProps) {
    this.render = DomBuilder.getInstance().build({
      type: 'textarea',
      testId: testIds.feedbackForm.comment,
      events: {
        keyup: (e: InputEvent) => {
          if (props.onChange !== undefined) {
            props.onChange(this.render.value);
          }
        },
      },
      classNames: [ 'mynah-feedback-form-comment' ],
    });

    // Set the initial value after creating the element
    this.render.value = props.initComment ?? '';
  }

  getComment = (): string => this.render.value;
  clear = (): void => { this.render.value = ''; };
}
