/*!
 * Copyright 2022 Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

import { DomBuilderObject, DS, ExtendedHTMLElement } from '../helper/dom';
import { cancelEvent } from '../helper/events';
import { NotificationType } from '../static';
import { Icon, MynahIcons } from './icon';
import { Overlay, OverlayHorizontalDirection, OverlayVerticalDirection, OVERLAY_MARGIN } from './overlay';
import testIds from '../helper/test-ids';
import { StyleLoader } from '../helper/style-loader';

type NotificationContentType = string | ExtendedHTMLElement | HTMLElement | DomBuilderObject;

export const DEFAULT_TIMEOUT = 5000;

export interface NotificationProps {
  duration?: number;
  type?: NotificationType;
  title?: string;
  content: NotificationContentType | NotificationContentType[];
  onNotificationClick?: () => void;
  onNotificationHide?: () => void;
}

export class Notification {
  private notificationOverlay!: Overlay;
  private readonly duration;
  private readonly type;
  private readonly props;

  constructor (props: NotificationProps) {
    StyleLoader.getInstance().load('components/_notification.scss');
    this.duration = props.duration ?? DEFAULT_TIMEOUT;
    this.type = props.type ?? NotificationType.INFO;
    this.props = props;
  }

  public notify (): void {
    this.notificationOverlay = new Overlay({
      referencePoint: {
        left: Math.max(document.documentElement.clientWidth ?? 0, window.innerWidth ?? 0),
        top: this.getNextCalculatedTop(),
      },
      dimOutside: false,
      closeOnOutsideClick: false,
      horizontalDirection: OverlayHorizontalDirection.TO_LEFT,
      verticalDirection: OverlayVerticalDirection.TO_BOTTOM,
      onClose: this.props.onNotificationHide,
      children: [
        {
          type: 'div',
          testId: testIds.notification.wrapper,
          classNames: [
            'mynah-notification',
            this.props.onNotificationClick != null ? 'mynah-notification-clickable' : '',
          ],
          events: {
            click: e => {
              cancelEvent(e);
              if (this.props.onNotificationClick != null) {
                this.props.onNotificationClick();
                this.notificationOverlay?.close();
              }
            },
          },
          children: [
            new Icon({ icon: this.type.toString() as MynahIcons }).render,
            {
              type: 'div',
              classNames: [ 'mynah-notification-container' ],
              children: [
                {
                  type: 'h3',
                  testId: testIds.notification.title,
                  classNames: [ 'mynah-notification-title' ],
                  children: [ this.props.title ?? '' ],
                },
                {
                  type: 'div',
                  testId: testIds.notification.content,
                  classNames: [ 'mynah-notification-content' ],
                  children: this.getChildren(this.props.content),
                },
              ],
            },
          ],
        },
      ],
    });

    if (this.duration !== -1) {
      setTimeout(() => {
        this.notificationOverlay?.close();
      }, this.duration);
    }
  }

  /**
   * Calculates the top according to the previously shown and still visible notifications
   * @returns number
   */
  private readonly getNextCalculatedTop = (): number => {
    const prevNotifications = DS('.mynah-notification');
    if (prevNotifications.length > 0) {
      const prevNotificationRectangle = prevNotifications[prevNotifications.length - 1].getBoundingClientRect();
      return prevNotificationRectangle.top + prevNotificationRectangle.height + OVERLAY_MARGIN;
    }
    return 0;
  };

  private readonly getChildren = (content: NotificationContentType | NotificationContentType[]): NotificationContentType[] => {
    if (content instanceof Array) {
      return content;
    }
    return [ content ];
  };
}
