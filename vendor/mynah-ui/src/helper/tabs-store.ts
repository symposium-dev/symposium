/*!
 * Copyright 2022 Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */
/* eslint-disable @typescript-eslint/no-dynamic-delete */
import clone from 'just-clone';
import { MynahUIDataModel, MynahUITabStoreModel, MynahUITabStoreTab } from '../static';
import { Config } from './config';
import { generateUID } from './guid';
import { MynahUIDataStore } from './store';

interface TabStoreSubscription {
  'add': Record<string, (tabId: string, tabData?: MynahUITabStoreTab) => void>;
  'remove': Record<string, (tabId: string, newSelectedTab?: MynahUITabStoreTab) => void>;
  'update': Record<string, (tabId: string, tabData?: MynahUITabStoreTab) => void>;
  'beforeTabChange': Record<string, (tabId: string, previousSelectedTab?: MynahUITabStoreTab) => void>;
  'selectedTabChange': Record<string, (tabId: string, previousSelectedTab?: MynahUITabStoreTab) => void>;
}
export class EmptyMynahUITabsStoreModel {
  data: Required<MynahUITabStoreModel>;
  constructor () {
    const guid = generateUID();
    this.data = {
      [guid]: {
        isSelected: true,
        store: {},
      }
    };
  }
}
export class MynahUITabsStore {
  private static instance: MynahUITabsStore | undefined;
  private readonly subscriptions: TabStoreSubscription = {
    add: {},
    remove: {},
    update: {},
    beforeTabChange: {},
    selectedTabChange: {}
  };

  private readonly tabsStore: Required<MynahUITabStoreModel> = {};
  private readonly tabsDataStore: Record<string, MynahUIDataStore> = {};
  private tabDefaults: MynahUITabStoreTab = {};

  private constructor (initialData?: MynahUITabStoreModel, defaults?: MynahUITabStoreTab) {
    this.tabsStore = Object.assign(this.tabsStore, initialData);
    if (defaults != null) {
      this.tabDefaults = defaults;
    }
    if ((initialData != null) && Object.keys(initialData).length > 0) {
      Object.keys(initialData).forEach((tabId: string) => {
        this.tabsDataStore[tabId] = new MynahUIDataStore(tabId, initialData[tabId].store ?? {});
      });
    }
  }

  private readonly deselectAllTabs = (): void => {
    Object.keys(this.tabsStore).forEach(tabId => {
      this.tabsStore[tabId].isSelected = false;
    });
  };

  public readonly addTab = (tabData?: MynahUITabStoreTab): string | undefined => {
    if (Object.keys(this.tabsStore).length < Config.getInstance().config.maxTabs) {
      const tabId = generateUID();
      this.deselectAllTabs();
      this.tabsStore[tabId] = { ...this.tabDefaults, ...tabData, isSelected: true };
      this.tabsDataStore[tabId] = new MynahUIDataStore(tabId, this.tabsStore[tabId].store ?? {});
      this.informSubscribers('add', tabId, this.tabsStore[tabId]);
      this.informSubscribers('selectedTabChange', tabId, this.tabsStore[tabId]);
      return tabId;
    }
  };

  public readonly removeTab = (tabId: string): string => {
    const wasSelected = this.tabsStore[tabId].isSelected ?? false;
    let newSelectedTab: MynahUITabStoreTab | undefined;
    delete this.tabsStore[tabId];
    this.tabsDataStore[tabId].resetStore();
    delete this.tabsDataStore[tabId];
    if (wasSelected) {
      const tabIds = Object.keys(this.tabsStore);
      if (tabIds.length > 0) {
        this.deselectAllTabs();
        this.selectTab(tabIds[tabIds.length - 1]);
        newSelectedTab = this.tabsStore[this.getSelectedTabId()];
      }
    }
    this.informSubscribers('remove', tabId, newSelectedTab);
    return tabId;
  };

  public readonly selectTab = (tabId: string): void => {
    this.informSubscribers('beforeTabChange', tabId, this.tabsStore[tabId]);
    this.deselectAllTabs();
    this.tabsStore[tabId].isSelected = true;
    this.informSubscribers('selectedTabChange', tabId, this.tabsStore[tabId]);
  };

  /**
   * Updates the store and informs the subscribers.
   * @param data A full or partial set of store data model with values.
   */
  public updateTab = (tabId: string, tabData?: Partial<MynahUITabStoreTab>, skipSubscribers?: boolean): void => {
    if (this.tabsStore[tabId] != null) {
      if (tabData?.isSelected === true && this.getSelectedTabId() !== tabId) {
        this.selectTab(tabId);
      }
      this.tabsStore[tabId] = { ...this.tabsStore[tabId], ...tabData };
      if (tabData?.store != null) {
        if (this.tabsDataStore[tabId] === undefined) {
          this.tabsDataStore[tabId] = new MynahUIDataStore(tabId);
        }
        this.tabsDataStore[tabId].updateStore(tabData?.store);
      }
      if (skipSubscribers !== true) {
        this.informSubscribers('update', tabId, this.tabsStore[tabId]);
      }
    }
  };

  public static getInstance = (initialData?: MynahUITabStoreModel, defaults?: MynahUITabStoreTab): MynahUITabsStore => {
    if (MynahUITabsStore.instance === undefined) {
      MynahUITabsStore.instance = new MynahUITabsStore(initialData, defaults);
    }

    return MynahUITabsStore.instance;
  };

  /**
   * Subscribe to changes of the tabsStore
   * @param handler function will be called when tabs changed
   * @returns subscriptionId which needed to unsubscribe
   */
  public addListener = (eventName: keyof TabStoreSubscription, handler: (tabId: string, tabData?: MynahUITabStoreTab) => void): string => {
    const subscriptionId: string = generateUID();
    this.subscriptions[eventName][subscriptionId] = handler;
    return subscriptionId;
  };

  /**
   * Subscribe to changes of the tabs' data store
   * @param handler function will be called when tabs changed
   * @returns subscriptionId which needed to unsubscribe
   */
  public addListenerToDataStore = (tabId: string, storeKey: keyof MynahUIDataModel, handler: (newValue: any, oldValue?: any) => void): string | null => {
    if (this.tabsDataStore[tabId] !== undefined) {
      return this.tabsDataStore[tabId].subscribe(storeKey, handler);
    }
    return null;
  };

  /**
   * Unsubscribe from changes of the tabs' data store
   * @param handler function will be called when tabs changed
   * @returns subscriptionId which needed to unsubscribe
   */
  public removeListenerFromDataStore = (tabId: string, subscriptionId: string, storeKey: keyof MynahUIDataModel): void => {
    if (this.tabsDataStore[tabId] !== undefined) {
      this.tabsDataStore[tabId].unsubscribe(storeKey, subscriptionId);
    }
  };

  /**
   * Unsubscribe from changes of the tabs store
   * @param subscriptionId subscriptionId which is returned from subscribe function
   */
  public removeListener = (eventName: keyof TabStoreSubscription, subscriptionId: string): void => {
    if (this.subscriptions[eventName][subscriptionId] !== undefined) {
      delete this.subscriptions[eventName][subscriptionId];
    }
  };

  private readonly informSubscribers = (eventName: keyof TabStoreSubscription, tabId: string, tabData?: MynahUITabStoreTab): void => {
    const subscriberKeys = Object.keys(this.subscriptions[eventName]);
    subscriberKeys.forEach(subscriberKey => {
      this.subscriptions[eventName][subscriberKey](tabId, tabData);
    });
  };

  /**
   * Returns the tab
   * @param tabId Tab Id
   * @returns info of the tab
   */
  public getTab = (tabId: string): MynahUITabStoreTab | null => this.tabsStore[tabId] ?? null;

  /**
   * Returns the tab
   * @param tabId Tab Id
   * @returns info of the tab
   */
  public getAllTabs = (): MynahUITabStoreModel => {
    const clonedTabs = clone(this.tabsStore) as MynahUITabStoreModel;
    Object.keys(clonedTabs).forEach(tabId => {
      clonedTabs[tabId].store = clone(this.getTabDataStore(tabId).getStore() as object) ?? {};
    });
    return clonedTabs;
  };

  /**
   * Returns the data store of the tab
   * @param tabId Tab Id
   * @returns data of the tab
   */
  public getTabDataStore = (tabId: string): MynahUIDataStore => this.tabsDataStore[tabId];

  /**
   * Returns the data of the tab
   * @param tabId Tab Id
   * @returns data of the tab
   */
  public getSelectedTabId = (): string => {
    const tabIds = Object.keys(this.tabsStore);
    return tabIds.find(tabId => this.tabsStore[tabId].isSelected === true) ?? '';
  };

  /**
   * Clears store data and informs all the subscribers
   */
  public removeAllTabs = (): void => {
    Object.keys(this.tabsStore).forEach(tabId => {
      this.removeTab(tabId);
    });
  };

  /**
   * Get all tabs length
   * @returns tabs length
   */
  public tabsLength = (): number => Object.keys(this.tabsStore).length;

  /**
   * Updates defaults of the tab store
   * @param defaults MynahUITabStoreTab
   */
  public updateTabDefaults = (defaults: MynahUITabStoreTab): void => {
    this.tabDefaults = {
      store: {
        ...this.tabDefaults.store,
        ...defaults.store
      }
    };
  };

  /**
   * Updates defaults of the tab store
   * @param defaults MynahUITabStoreTab
   */
  public getTabDefaults = (): MynahUITabStoreTab => {
    return this.tabDefaults;
  };

  public destroy = (): void => {
    MynahUITabsStore.instance = undefined;
  };
}
