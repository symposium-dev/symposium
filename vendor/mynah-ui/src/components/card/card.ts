/*!
 * Copyright 2022 Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */
import { DomBuilder, DomBuilderObject, ExtendedHTMLElement } from '../../helper/dom';
import { StyleLoader } from '../../helper/style-loader';
import { EngagementType, Status } from '../../static';

/**
 * We'll not consider it as an engagement if the total spend time is lower than below constant and won't trigger the event
 */
const ENGAGEMENT_DURATION_LIMIT = 3000;

/**
 * This 6(px) and 300(ms) are coming from a behavioral research and browser reaction to input devices to count the action as a mouse movement or a click event
 */
const ENGAGEMENT_MIN_SELECTION_DISTANCE = 6;
const ENGAGEMENT_MIN_CLICK_DURATION = 300;
export interface CardProps extends Partial<DomBuilderObject> {
  border?: boolean;
  background?: boolean;
  status?: Status;
  padding?: 'small' | 'medium' | 'large' | 'none';
  children?: Array<HTMLElement | ExtendedHTMLElement | string>;
  onCardEngaged?: (engagement: {
    engagementDurationTillTrigger: number;
    engagementType: EngagementType;
    totalMouseDistanceTraveled: {
      x: number;
      y: number;
    };
    selectionDistanceTraveled?: { x: number; y: number; selectedText?: string | undefined };
  }) => void;
}
export class Card {
  render: ExtendedHTMLElement;
  private readonly props: CardProps;
  private engagementStartTime: number = -1;
  private totalMouseDistanceTraveled: { x: number; y: number } = { x: 0, y: 0 };
  private previousMousePosition!: { x: number; y: number };
  private mouseDownInfo!: { x: number; y: number; time: number };
  constructor (props: CardProps) {
    StyleLoader.getInstance().load('components/card/_card.scss');
    this.props = props;
    this.render = DomBuilder.getInstance().build({
      type: 'div',
      testId: this.props.testId,
      classNames: [
        'mynah-card',
        `padding-${props.padding ?? 'medium'}`,
        `status-${props.status ?? 'default'}`,
        props.border !== false ? 'border' : '',
        props.background !== false ? 'background' : '',
        ...(props.classNames ?? [])
      ],
      persistent: props.persistent,
      innerHTML: props.innerHTML,
      children: [
        ...(props.children ?? []),
      ],
      events: {
        ...props.events,
        ...(props.onCardEngaged !== undefined
          ? {
              mouseenter: e => {
                if (this.engagementStartTime === -1) {
                  this.engagementStartTime = new Date().getTime();
                  this.previousMousePosition = { x: e.clientX, y: e.clientY };
                  this.totalMouseDistanceTraveled = { x: 0, y: 0 };
                }
              },
              mousemove: e => {
                if (this.engagementStartTime === -1) {
                  this.engagementStartTime = new Date().getTime();
                }
                this.totalMouseDistanceTraveled = {
                  x:
                          this.totalMouseDistanceTraveled.x +
                          Math.abs(e.clientX - this.previousMousePosition.x),
                  y:
                          this.totalMouseDistanceTraveled.y +
                          Math.abs(e.clientY - this.previousMousePosition.y),
                };
                this.previousMousePosition = { x: e.clientX, y: e.clientY };
              },
              mousedown: e => {
                this.mouseDownInfo = { x: e.clientX, y: e.clientY, time: new Date().getTime() };
              },
              mouseup: e => {
                const mouseUpInfo = { x: e.clientX, y: e.clientY, time: new Date().getTime() };
                if (
                  this.mouseDownInfo !== undefined &&
                      (Math.abs(this.mouseDownInfo.x - mouseUpInfo.x) > ENGAGEMENT_MIN_SELECTION_DISTANCE ||
                          Math.abs(this.mouseDownInfo.y - mouseUpInfo.y) >
                          ENGAGEMENT_MIN_SELECTION_DISTANCE) &&
                      mouseUpInfo.time - this.mouseDownInfo.time > ENGAGEMENT_MIN_CLICK_DURATION
                ) {
                  this.handleEngagement({
                    x: Math.abs(this.mouseDownInfo.x - mouseUpInfo.x),
                    y: Math.abs(this.mouseDownInfo.y - mouseUpInfo.y),
                    selectedText: window?.getSelection()?.toString(),
                  });
                }
              },
              mouseleave: () => {
                const engagementEndTime = new Date().getTime();
                if (this.engagementStartTime !== -1 && engagementEndTime - this.engagementStartTime > ENGAGEMENT_DURATION_LIMIT) {
                  this.handleEngagement();
                } else {
                  this.resetEngagement();
                }
              },
            }
          : {})
      },
      attributes: props.attributes
    });
  }

  private readonly resetEngagement = (): void => {
    this.engagementStartTime = -1;
    this.totalMouseDistanceTraveled = { x: 0, y: 0 };
    this.previousMousePosition = { x: 0, y: 0 };
    this.mouseDownInfo = { x: 0, y: 0, time: -1 };
  };

  private readonly handleEngagement = (interactionDistanceTraveled?: {
    x: number;
    y: number;
    selectedText?: string;
  }): void => {
    if (this.props.onCardEngaged !== undefined) {
      this.props.onCardEngaged({
        engagementDurationTillTrigger: new Date().getTime() - this.engagementStartTime,
        engagementType:
                    interactionDistanceTraveled !== undefined ? EngagementType.INTERACTION : EngagementType.TIME,
        totalMouseDistanceTraveled: this.totalMouseDistanceTraveled,
        selectionDistanceTraveled:
                    Boolean(interactionDistanceTraveled?.x ?? 0) && Boolean(interactionDistanceTraveled?.y)
                      ? interactionDistanceTraveled
                      : undefined,
      });
    }
    this.resetEngagement();
  };
}
