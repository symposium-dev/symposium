/*!
 * Copyright 2022 Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */
/* eslint-disable @typescript-eslint/no-dynamic-delete */
import { MynahEventNames } from '../static';
import { generateUID } from './guid';

export const cancelEvent = (event: Event): boolean => {
  event.preventDefault?.();
  event.stopPropagation?.();
  event.stopImmediatePropagation?.();
  return false;
};

export class MynahUIGlobalEvents {
  private static instance: MynahUIGlobalEvents | undefined;
  private readonly listeners: Record<MynahEventNames, Record<string, (value?: any) => void>>;

  private constructor () {
    this.listeners = { ...this.listeners };
  }

  public static getInstance = (): MynahUIGlobalEvents => {
    if (MynahUIGlobalEvents.instance === undefined) {
      MynahUIGlobalEvents.instance = new MynahUIGlobalEvents();
    }

    return MynahUIGlobalEvents.instance;
  };

  /**
   * Subscribe to value changes of a specific item in data store
   * @param eventKey One of the keys in MynahUIDataModel
   * @param handler function will be called with optional data field
   * @returns listenerId which will be necessary to removeListener
   */
  public addListener = (eventKey: MynahEventNames, handler: (data?: any) => void): string => {
    const listenerId: string = generateUID();
    if (this.listeners[eventKey] === undefined) {
      this.listeners[eventKey] = {};
    }
    this.listeners[eventKey][listenerId] = handler;
    return listenerId;
  };

  /**
   * Unsubscribe from changes of a specific item in data store
   * @param eventKey One of the keys in MynahUIDataModel
   * @param listenerId listenerId which is returned from addListener function
   */
  public removeListener = (eventKey: MynahEventNames, listenerId: string): void => {
    if (this.listeners[eventKey]?.[listenerId] !== undefined) {
      delete this.listeners[eventKey][listenerId];
    }
  };

  /**
   * Updates the store and informs the subscribers.
   * @param data A full or partial set of store data model with values.
   */
  public dispatch = (eventKey: MynahEventNames, data?: any): void => {
    if (this.listeners[eventKey] !== undefined) {
      Object.keys(this.listeners[eventKey]).forEach((listenerId: string) => {
        this.listeners[eventKey][listenerId](data);
      });
    }
  };

  public destroy = (): void => {
    MynahUIGlobalEvents.instance = undefined;
  };
}
