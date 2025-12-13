/*!
 * Copyright 2022 Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

import { Config } from '../helper/config';
import { DomBuilder, ExtendedHTMLElement } from '../helper/dom';
import { cancelEvent } from '../helper/events';
import { MynahUITabsStore } from '../helper/tabs-store';
import { MynahUITabStoreTab } from '../static';
import { Button } from './button';
import { Card } from './card/card';
import { CardBody } from './card/card-body';
import { Icon, MynahIcons } from './icon';
import { TabBarButtonsWrapper } from './navigation-tab-bar-buttons';
import { Overlay, OverlayHorizontalDirection, OverlayVerticalDirection } from './overlay';
import { Tab, ToggleOption } from './tabs';
import { DEFAULT_TIMEOUT } from './notification';
import testIds from '../helper/test-ids';
import { StyleLoader } from '../helper/style-loader';

export interface TabsProps {
  onChange?: (selectedTabId: string) => void;
  noMoreTabsTooltip?: string;
  onBeforeTabRemove?: (tabId: string) => boolean;
  maxTabsTooltipDuration?: number;
}
export class Tabs {
  render: ExtendedHTMLElement;
  private tabIdTitleSubscriptions: Record<string, string> = {};
  private tabIdChatItemsSubscriptions: Record<string, string> = {};
  private toggleGroup: Tab;
  private maxReachedOverlay: Overlay | undefined;
  private closeConfirmationOverlay: Overlay | undefined;
  private readonly props: TabsProps;

  constructor (props: TabsProps) {
    StyleLoader.getInstance().load('components/_nav-tabs.scss');
    this.props = props;
    this.render = DomBuilder.getInstance().build({
      type: 'div',
      testId: testIds.tabBar.wrapper,
      persistent: true,
      classNames: [ 'mynah-nav-tabs-wrapper' ],
      events: {
        dblclick: (e) => {
          cancelEvent(e);
          if (MynahUITabsStore.getInstance().tabsLength() < Config.getInstance().config.maxTabs) {
            MynahUITabsStore.getInstance().addTab();
          }
        }
      },
      children: [
        ...this.getTabsRender(MynahUITabsStore.getInstance().getSelectedTabId()),
        new TabBarButtonsWrapper().render
      ],
    });

    MynahUITabsStore.getInstance().addListener('add', (tabId, tabData) => {
      this.assignListener(tabId);
      this.toggleGroup.addOption({
        value: tabId,
        label: tabData?.store?.tabTitle,
        selected: tabData?.isSelected
      });
      this.render.setAttribute('selected-tab', tabId);
    });
    MynahUITabsStore.getInstance().addListener('remove', (tabId, newSelectedTab?: MynahUITabStoreTab) => {
      this.removeListenerAssignments(tabId);
      this.toggleGroup.removeOption(tabId);
      if (newSelectedTab !== undefined) {
        this.toggleGroup.snapToOption(MynahUITabsStore.getInstance().getSelectedTabId());
      }
      this.render.setAttribute('selected-tab', MynahUITabsStore.getInstance().getSelectedTabId());
    });
    MynahUITabsStore.getInstance().addListener('selectedTabChange', (selectedTabId) => {
      this.render.setAttribute('selected-tab', selectedTabId);
      this.toggleGroup.setValue(selectedTabId);
    });
  }

  private readonly getTabOptionsFromTabStoreData = (): ToggleOption[] => {
    const tabs = MynahUITabsStore.getInstance().getAllTabs();
    return Object.keys(tabs).map((tabId: string) => {
      const tabOption = {
        value: tabId,
        label: tabs[tabId].store?.tabTitle,
        icon: tabs[tabId].store?.tabIcon,
        pinned: tabs[tabId].store?.pinned,
        selected: tabs[tabId].isSelected
      };
      return tabOption;
    });
  };

  private readonly getTabsRender = (selectedTabId?: string): ExtendedHTMLElement[] => {
    const tabs = this.getTabOptionsFromTabStoreData();
    tabs.forEach(tab => {
      this.assignListener(tab.value);
    });
    this.toggleGroup = new Tab({
      testId: testIds.tabBar.tabsWrapper,
      onChange: (selectedTabId: string) => {
        MynahUITabsStore.getInstance().selectTab(selectedTabId);
        if (this.props.onChange !== undefined) {
          this.props.onChange(selectedTabId);
        }
      },
      onRemove: (selectedTabId, domElement: ExtendedHTMLElement) => {
        if (this.props.onBeforeTabRemove !== undefined && !this.props.onBeforeTabRemove(selectedTabId)) {
          this.showCloseTabConfirmationOverLay(domElement, selectedTabId);
        } else {
          MynahUITabsStore.getInstance().removeTab(selectedTabId);
        }
      },
      name: 'mynah-main-tabs',
      options: tabs,
      value: selectedTabId
    });
    return [
      this.toggleGroup.render,
      new Button({
        testId: testIds.tabBar.tabAddButton,
        classNames: [ 'mynah-tabs-close-button' ],
        additionalEvents: {
          mouseenter: (e) => {
            if (MynahUITabsStore.getInstance().tabsLength() === Config.getInstance().config.maxTabs) {
              this.showMaxReachedOverLay(e.currentTarget, this.props.noMoreTabsTooltip ?? Config.getInstance().config.texts.noMoreTabsTooltip, this.props.maxTabsTooltipDuration);
            }
          },
          mouseleave: () => {
            this.hideMaxReachedOverLay();
          },
        },
        onClick: (e) => {
          cancelEvent(e);
          if (MynahUITabsStore.getInstance().tabsLength() < Config.getInstance().config.maxTabs) {
            MynahUITabsStore.getInstance().addTab();
          }
        },
        icon: new Icon({ icon: MynahIcons.PLUS }).render,
        primary: false
      }).render
    ];
  };

  private readonly showMaxReachedOverLay = (elm: HTMLElement, markdownText: string, duration?: number): void => {
    this.maxReachedOverlay = new Overlay({
      testId: testIds.tabBar.maxTabsReachedOverlay,
      background: true,
      closeOnOutsideClick: false,
      referenceElement: elm,
      dimOutside: false,
      removeOtherOverlays: true,
      verticalDirection: OverlayVerticalDirection.TO_BOTTOM,
      horizontalDirection: OverlayHorizontalDirection.CENTER,
      children: [
        new Card({
          border: false,
          classNames: [ 'mynah-nav-tabs-max-reached-overlay' ],
          children: [
            new CardBody({
              body: markdownText,
            }).render,
          ]
        }).render
      ],
    });

    if (duration !== undefined && duration !== -1) {
      setTimeout(() => {
        this.hideMaxReachedOverLay();
      }, duration);
    } else if (duration === undefined) {
      setTimeout(() => {
        this.hideMaxReachedOverLay();
      }, DEFAULT_TIMEOUT);
    }
  };

  private readonly hideMaxReachedOverLay = (): void => {
    if (this.maxReachedOverlay !== undefined) {
      this.maxReachedOverlay.close();
      this.maxReachedOverlay = undefined;
    }
  };

  private readonly showCloseTabConfirmationOverLay = (elm: HTMLElement, selectedTabId: string): void => {
    this.closeConfirmationOverlay = new Overlay({
      testId: testIds.tabBar.tabCloseConfirmationOverlay,
      background: true,
      closeOnOutsideClick: true,
      referenceElement: elm,
      dimOutside: false,
      removeOtherOverlays: true,
      verticalDirection: OverlayVerticalDirection.TO_BOTTOM,
      horizontalDirection: OverlayHorizontalDirection.START_TO_RIGHT,
      children: [
        new Card({
          border: false,
          classNames: [ 'mynah-nav-tabs-close-confirmation-overlay' ],
          children: [
            new CardBody({
              testId: testIds.tabBar.tabCloseConfirmationBody,
              body: MynahUITabsStore.getInstance().getTabDataStore(selectedTabId).getValue('tabCloseConfirmationMessage') ??
              Config.getInstance().config.texts.tabCloseConfirmationMessage,
            }).render,
            DomBuilder.getInstance().build({
              type: 'div',
              classNames: [ 'mynah-nav-tabs-close-confirmation-buttons-wrapper' ],
              children: [
                new Button({
                  testId: testIds.tabBar.tabCloseConfirmationCancelButton,
                  onClick: () => {
                    this.hideshowCloseTabConfirmationOverLay();
                  },
                  label: MynahUITabsStore.getInstance().getTabDataStore(selectedTabId).getValue('tabCloseConfirmationKeepButton') ??
                  Config.getInstance().config.texts.tabCloseConfirmationKeepButton
                }).render,
                new Button({
                  testId: testIds.tabBar.tabCloseConfirmationAcceptButton,
                  onClick: () => {
                    MynahUITabsStore.getInstance().removeTab(selectedTabId);
                    this.hideshowCloseTabConfirmationOverLay();
                  },
                  classNames: [ 'mynah-nav-tabs-close-confirmation-close-button' ],
                  label: MynahUITabsStore.getInstance().getTabDataStore(selectedTabId).getValue('tabCloseConfirmationCloseButton') ??
                  Config.getInstance().config.texts.tabCloseConfirmationCloseButton
                }).render,
              ]
            })
          ]
        }).render
      ],
    });
  };

  private readonly hideshowCloseTabConfirmationOverLay = (): void => {
    if (this.closeConfirmationOverlay !== undefined) {
      this.closeConfirmationOverlay.close();
      this.closeConfirmationOverlay = undefined;
    }
  };

  private readonly assignListener = (tabId: string): void => {
    this.tabIdTitleSubscriptions[tabId] = MynahUITabsStore.getInstance().addListenerToDataStore(tabId, 'tabTitle', (title) => {
      this.toggleGroup.updateOptionTitle(tabId, title);
    }) ?? '';
    this.tabIdChatItemsSubscriptions[tabId] = MynahUITabsStore.getInstance().addListenerToDataStore(tabId, 'chatItems', () => {
      this.toggleGroup.updateOptionIndicator(tabId, true);
    }) ?? '';
  };

  private readonly removeListenerAssignments = (tabId: string): void => {
    MynahUITabsStore.getInstance().removeListenerFromDataStore(tabId, this.tabIdTitleSubscriptions[tabId], 'tabTitle');
    MynahUITabsStore.getInstance().removeListenerFromDataStore(tabId, this.tabIdChatItemsSubscriptions[tabId], 'chatItems');
  };
}
