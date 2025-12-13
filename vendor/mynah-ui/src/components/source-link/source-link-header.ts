/*!
 * Copyright 2022 Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

import { getTimeDiff } from '../../helper/date-time';
import { DomBuilder, DomBuilderObject, ExtendedHTMLElement } from '../../helper/dom';
import { MynahUIGlobalEvents } from '../../helper/events';
import testIds from '../../helper/test-ids';
import { getOrigin } from '../../helper/url';
import { MynahEventNames, SourceLink, SourceLinkMetaData } from '../../static';
import { Icon, MynahIcons } from '../icon';
import { Overlay, OverlayHorizontalDirection, OverlayVerticalDirection } from '../overlay';
import { SourceLinkCard } from './source-link';

const PREVIEW_DELAY = 500;
export interface SourceLinkHeaderProps {
  sourceLink: SourceLink;
  showCardOnHover?: boolean;
  onClick?: (e?: MouseEvent) => void;
}
export class SourceLinkHeader {
  private sourceLinkPreview: Overlay | null;
  private sourceLinkPreviewTimeout: ReturnType<typeof setTimeout>;
  render: ExtendedHTMLElement;
  constructor (props: SourceLinkHeaderProps) {
    const splitUrl = props.sourceLink.url
      .replace(/^(http|https):\/\//, '')
      .split('/');
    if (splitUrl[splitUrl.length - 1].trim() === '') {
      splitUrl.pop();
    }
    MynahUIGlobalEvents.getInstance().addListener(MynahEventNames.ROOT_FOCUS, (data: {focusState: boolean}) => {
      if (!data.focusState) {
        this.hideLinkPreview();
      }
    });
    this.render = DomBuilder.getInstance().build({
      type: 'div',
      testId: testIds.chatItem.relatedLinks.linkWrapper,
      classNames: [ 'mynah-source-link-header' ],
      ...(props.showCardOnHover === true
        ? {
            events: {
              mouseenter: (e) => {
                this.showLinkPreview(e, props.sourceLink);
              },
              mouseleave: this.hideLinkPreview,
              focus: (e) => {
                this.showLinkPreview(e, props.sourceLink);
              },
              blur: this.hideLinkPreview,
            }
          }
        : {}),
      attributes: {
        origin: getOrigin(props.sourceLink.url)
      },
      children: [
        {
          type: 'span',
          classNames: [ 'mynah-source-thumbnail' ]
        },
        {
          type: 'div',
          classNames: [ 'mynah-source-link-title-wrapper' ],
          children: [
            {
              type: 'a',
              classNames: [ 'mynah-source-link-title' ],
              events: {
                ...(props.onClick !== undefined && {
                  click: props.onClick,
                  auxclick: props.onClick,
                }),
              },
              attributes: { href: props.sourceLink.url, target: '_blank' },
              children: [ props.sourceLink.title, {
                type: 'div',
                classNames: [ 'mynah-source-link-expand-icon' ],
                children: [ new Icon({ icon: MynahIcons.EXTERNAL }).render ],
              } ],
            },
            {
              type: 'a',
              testId: testIds.chatItem.relatedLinks.link,
              classNames: [ 'mynah-source-link-url' ],
              events: {
                ...(props.onClick !== undefined && {
                  click: props.onClick,
                  auxclick: props.onClick
                }),
              },
              attributes: { href: props.sourceLink.url, target: '_blank' },
              innerHTML: splitUrl.map(urlPart => `<span>${urlPart}</span>`).join(''),
            },
            ...((props.sourceLink.metadata != null) ? [ this.getSourceMetaBlock(props.sourceLink.metadata) ] : []),
          ],
        },
      ],
    });
  }

  private readonly getSourceMetaBlock = (metadataUnion?: Record<string, SourceLinkMetaData>): DomBuilderObject => {
    const metaItems: any[] = [];
    if (metadataUnion !== null && metadataUnion !== undefined) {
      Object.keys(metadataUnion).forEach(metadataKey => {
        const metadata = metadataUnion[metadataKey];
        if (metadata.isAccepted === true) {
          metaItems.push({
            type: 'span',
            classNames: [ 'mynah-title-meta-block-item', 'approved-answer' ],
            children: [
              new Icon({ icon: MynahIcons.OK }).render,
            ]
          });
        }

        if (metadata.lastActivityDate !== undefined) {
          metaItems.push({
            type: 'span',
            classNames: [ 'mynah-title-meta-block-item' ],
            children: [
              new Icon({ icon: MynahIcons.CALENDAR }).render,
              {
                type: 'span',
                classNames: [ 'mynah-title-meta-block-item-text' ],
                children: [ getTimeDiff((new Date()).getTime() - metadata.lastActivityDate, 2) ]
              }
            ]
          });
        }

        if (metadata.answerCount !== undefined) {
          metaItems.push({
            type: 'span',
            classNames: [ 'mynah-title-meta-block-item' ],
            children: [
              new Icon({ icon: MynahIcons.CHAT }).render,
              {
                type: 'span',
                classNames: [ 'mynah-title-meta-block-item-text' ],
                children: [ metadata.answerCount.toString() ]
              }
            ]
          });
        }

        if (metadata.stars !== undefined) {
          metaItems.push({
            type: 'span',
            classNames: [ 'mynah-title-meta-block-item' ],
            children: [
              new Icon({ icon: MynahIcons.STAR }).render,
              {
                type: 'span',
                classNames: [ 'mynah-title-meta-block-item-text' ],
                children: [ `${metadata.stars.toString()} contributors` ]
              }
            ]
          });
        }

        if (metadata.forks !== undefined) {
          metaItems.push({
            type: 'span',
            classNames: [ 'mynah-title-meta-block-item' ],
            children: [
              new Icon({ icon: MynahIcons.DOWN_OPEN }).render,
              {
                type: 'span',
                classNames: [ 'mynah-title-meta-block-item-text' ],
                children: [ `${metadata.forks.toString()} forks` ]
              }
            ]
          });
        }

        if (metadata.score !== undefined) {
          metaItems.push({
            type: 'span',
            classNames: [ 'mynah-title-meta-block-item' ],
            children: [
              new Icon({ icon: MynahIcons.THUMBS_UP }).render,
              {
                type: 'span',
                classNames: [ 'mynah-title-meta-block-item-text' ],
                children: [ `${metadata.score.toString()}` ]
              }
            ]
          });
        }
      });
    }

    return {
      type: 'span',
      classNames: [ 'mynah-title-meta-block' ],
      children: metaItems
    };
  };

  private readonly showLinkPreview = (e: MouseEvent, sourceLink: SourceLink): void => {
    if (sourceLink.body !== undefined) {
      clearTimeout(this.sourceLinkPreviewTimeout);
      this.sourceLinkPreviewTimeout = setTimeout(() => {
        const elm: HTMLElement = e.target as HTMLElement;
        this.sourceLinkPreview = new Overlay({
          testId: testIds.chatItem.relatedLinks.linkPreviewOverlay,
          background: true,
          closeOnOutsideClick: false,
          referenceElement: elm,
          dimOutside: false,
          removeOtherOverlays: true,
          verticalDirection: OverlayVerticalDirection.TO_TOP,
          horizontalDirection: OverlayHorizontalDirection.START_TO_RIGHT,
          children: [
            new SourceLinkCard({ sourceLink }).render
          ],
        });
      }, PREVIEW_DELAY);
    }
  };

  private readonly hideLinkPreview = (): void => {
    clearTimeout(this.sourceLinkPreviewTimeout);
    if (this.sourceLinkPreview !== null) {
      this.sourceLinkPreview?.close();
      this.sourceLinkPreview = null;
    }
  };
}
