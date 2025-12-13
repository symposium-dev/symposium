/*!
 * Copyright 2022 Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

import { Config } from '../helper/config';
import { DomBuilder, ExtendedHTMLElement } from '../helper/dom';
import { MynahUIGlobalEvents } from '../helper/events';
import { MynahUITabsStore } from '../helper/tabs-store';
import testIds from '../helper/test-ids';
import { MynahEventNames, TabBarAction, TabBarMainAction } from '../static';
import { Button } from './button';
import { Icon } from './icon';
import { Overlay, OverlayHorizontalDirection, OverlayVerticalDirection } from './overlay';

export interface TabBarButtonsWrapperProps {
  onButtonClick?: (selectedTabId: string, buttonId: string) => void;
}
export class TabBarButtonsWrapper {
  render: ExtendedHTMLElement;
  private selectedTabId: string;
  private tabBarButtonsSubscription: {subsId: string | null; tabId: string} | null = null;
  private readonly props: TabBarButtonsWrapperProps;

  constructor (props?: TabBarButtonsWrapperProps) {
    this.props = props ?? {};
    this.selectedTabId = MynahUITabsStore.getInstance().getSelectedTabId();
    this.handleTabBarButtonsChange();
    this.render = DomBuilder.getInstance().build({
      type: 'div',
      testId: testIds.tabBar.buttonsWrapper,
      persistent: true,
      classNames: [ 'mynah-nav-tabs-bar-buttons-wrapper' ],
      children: this.getTabsBarButtonsRender(this.selectedTabId),
    });

    MynahUITabsStore.getInstance().addListener('selectedTabChange', (selectedTabId) => {
      this.selectedTabId = selectedTabId;
      this.handleTabBarButtonsChange();
      this.render.clear();
      this.render.update({
        children: this.getTabsBarButtonsRender(selectedTabId)
      });
    });
  }

  private readonly handleTabBarButtonsChange = (): void => {
    if (this.tabBarButtonsSubscription?.subsId != null) {
      MynahUITabsStore.getInstance().removeListenerFromDataStore(this.tabBarButtonsSubscription.tabId, this.tabBarButtonsSubscription.subsId, 'tabBarButtons');
    }
    this.tabBarButtonsSubscription = {
      subsId: MynahUITabsStore.getInstance().addListenerToDataStore(this.selectedTabId, 'tabBarButtons', (tabBarButtons) => {
        this.render.clear();
        this.render.update({
          children: this.getTabsBarButtonsRender(this.selectedTabId, tabBarButtons)
        });
      }),
      tabId: this.selectedTabId
    };
  };

  private readonly getTabsBarButtonsRender = (selectedTabId: string, givenTabBarButtons?: TabBarMainAction[]): ExtendedHTMLElement[] => {
    let tabBarButtons = Config.getInstance().config.tabBarButtons ?? [];
    if (givenTabBarButtons != null) {
      tabBarButtons = givenTabBarButtons;
    } else {
      const tabBarButtonsFromTabStore = MynahUITabsStore.getInstance().getTabDataStore(selectedTabId)?.getValue('tabBarButtons');
      if (tabBarButtonsFromTabStore != null && tabBarButtonsFromTabStore.length > 0) {
        tabBarButtons = tabBarButtonsFromTabStore;
      }
    }
    return tabBarButtons.map((tabBarButton: TabBarMainAction) => new TabBarButtonWithMultipleOptions({
      onButtonClick: (tabBarAction) => {
        MynahUIGlobalEvents.getInstance().dispatch(MynahEventNames.TAB_BAR_BUTTON_CLICK, { tabId: selectedTabId, buttonId: tabBarAction.id });
        if (this.props.onButtonClick != null) {
          this.props.onButtonClick(selectedTabId, tabBarAction.id);
        }
      },
      tabBarActionButton: tabBarButton
    }).render);
  };
}

interface TabBarButtonWithMultipleOptionsProps {
  onButtonClick: (action: TabBarAction) => void;
  tabBarActionButton: TabBarMainAction;
}
export class TabBarButtonWithMultipleOptions {
  render: ExtendedHTMLElement;
  private buttonOptionsOverlay: Overlay | undefined;
  private readonly props: TabBarButtonWithMultipleOptionsProps;

  constructor (props: TabBarButtonWithMultipleOptionsProps) {
    this.props = props;
    this.render = new Button({
      testId: (this.props.tabBarActionButton.items != null && this.props.tabBarActionButton.items?.length > 0) ? testIds.tabBar.menuButton : testIds.tabBar.button,
      label: this.props.tabBarActionButton.text,
      tooltip: this.props.tabBarActionButton.description,
      // confirmation: this.props.tabBarActionButton.confirmation,
      disabled: this.props.tabBarActionButton.disabled,
      tooltipVerticalDirection: OverlayVerticalDirection.TO_BOTTOM,
      tooltipHorizontalDirection: OverlayHorizontalDirection.CENTER,
      icon: this.props.tabBarActionButton.icon != null ? new Icon({ icon: this.props.tabBarActionButton.icon }).render : undefined,
      primary: false,
      onClick: () => {
        if (this.props.tabBarActionButton.items != null && this.props.tabBarActionButton.items?.length > 0) {
          this.showButtonOptionsOverlay(this.props.tabBarActionButton.items);
        } else {
          this.props.onButtonClick(this.props.tabBarActionButton);
        }
      }
    }).render;
  }

  private readonly showButtonOptionsOverlay = (items: TabBarAction[]): void => {
    this.buttonOptionsOverlay = new Overlay({
      background: true,
      closeOnOutsideClick: true,
      referenceElement: this.render,
      dimOutside: false,
      removeOtherOverlays: true,
      verticalDirection: OverlayVerticalDirection.TO_BOTTOM,
      horizontalDirection: OverlayHorizontalDirection.END_TO_LEFT,
      children: [
        {
          type: 'div',
          classNames: [ 'mynah-nav-tabs-bar-buttons-wrapper-overlay' ],
          children: items.map(item => new Button({
            testId: testIds.tabBar.menuOption,
            confirmation: item.confirmation,
            label: item.text,
            icon: item.icon != null ? new Icon({ icon: item.icon }).render : undefined,
            primary: false,
            onClick: () => {
              this.hideButtonOptionsOverlay();
              this.props.onButtonClick(item);
            }
          }).render)
        }
      ],
    });
  };

  private readonly hideButtonOptionsOverlay = (): void => {
    if (this.buttonOptionsOverlay !== undefined) {
      this.buttonOptionsOverlay.close();
      this.buttonOptionsOverlay = undefined;
    }
  };
}
