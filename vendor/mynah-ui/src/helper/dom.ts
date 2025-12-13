/*!
 * Copyright 2022 Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

import testIds from '../helper/test-ids';
import { MynahEventNames, MynahPortalNames } from '../static';
import { Config } from './config';
import { MynahUIGlobalEvents } from './events';
import { AllowedTagsInCustomRenderer, AllowedAttributesInCustomRenderer } from './sanitize';

/* eslint-disable @typescript-eslint/method-signature-style */
/* eslint-disable @typescript-eslint/consistent-type-assertions */
/* eslint-disable @typescript-eslint/prefer-optional-chain */
/* eslint-disable @typescript-eslint/strict-boolean-expressions */
export const DS: typeof document.querySelectorAll = document.querySelectorAll.bind(document);

export type GenericEvents = Extract<keyof GlobalEventHandlersEventMap, string>;
export type DomBuilderEventHandler = (event?: any) => any;
export interface DomBuilderEventHandlerWithOptions {
  handler: DomBuilderEventHandler;
  options?: AddEventListenerOptions;
}
interface GenericDomBuilderAttributes {
  attributes?: Record<string, string | boolean> | undefined;
  classNames?: string[] | undefined;
  events?: Partial<Record<GenericEvents, DomBuilderEventHandler | DomBuilderEventHandlerWithOptions>> | undefined;
}

export interface ChatItemBodyRenderer extends GenericDomBuilderAttributes {
  type: AllowedTagsInCustomRenderer;
  children?: Array<string | ChatItemBodyRenderer> | undefined;
  attributes?: Partial<Record<AllowedAttributesInCustomRenderer, string>> | undefined;
}

export interface DomBuilderObject extends GenericDomBuilderAttributes{
  type: string;
  children?: Array<string | DomBuilderObject | HTMLElement | ExtendedHTMLElement> | undefined;
  innerHTML?: string | undefined;
  testId?: string;
  persistent?: boolean | undefined;
}

export interface DomBuilderObjectFilled {
  attributes?: Record<string, string | undefined>;
  classNames?: string[];
  events?: Record<string, (event?: any) => any>;
  children?: Array<string | DomBuilderObject | HTMLElement | ExtendedHTMLElement>;
  innerHTML?: string | undefined;
  testId?: string;
  persistent?: boolean;
}

const EmptyDomBuilderObject: DomBuilderObject = {
  type: 'div',
  attributes: {},
  classNames: [],
  events: {},
  children: [],
  innerHTML: undefined,
  persistent: false,
};

export interface ExtendedHTMLElement extends HTMLInputElement {
  addClass(className: string): ExtendedHTMLElement;
  removeClass(className: string): ExtendedHTMLElement;
  toggleClass(className: string): ExtendedHTMLElement;
  hasClass(className: string): boolean;
  insertChild(
    position: 'beforebegin' | 'afterbegin' | 'beforeend' | 'afterend',
    child: string | DomBuilderObject | HTMLElement | ExtendedHTMLElement | Array<string | DomBuilderObject | HTMLElement | ExtendedHTMLElement>
  ): ExtendedHTMLElement;
  clear(removePersistent?: boolean): ExtendedHTMLElement;
  builderObject: DomBuilderObject;
  update(builderObject: DomBuilderObjectFilled): ExtendedHTMLElement;
}

export class DomBuilder {
  private static instance: DomBuilder | undefined;
  private rootFocus: boolean;
  private readonly resizeObserver: ResizeObserver;
  private rootBox: DOMRect;
  root: ExtendedHTMLElement;
  private portals: Record<string, ExtendedHTMLElement> = {};

  private constructor (rootSelector: string) {
    this.root = DS(rootSelector)[0] as ExtendedHTMLElement;
    this.extendDomFunctionality(this.root);
    this.root.addClass('mynah-ui-root');
    this.rootFocus = this.root.matches(':focus') ?? false;
    this.attachRootFocusListeners();
    if (ResizeObserver != null) {
      this.rootBox = this.root.getBoundingClientRect();
      this.resizeObserver = new ResizeObserver((entry) => {
        const incomingRootBox = this.root.getBoundingClientRect();
        // Known issue of ResizeObserver, triggers twice for each size change.
        // Check if size was really changed then trigger
        if (this.rootBox.height !== incomingRootBox.height || this.rootBox.width !== incomingRootBox.width) {
          this.rootBox = incomingRootBox;
          MynahUIGlobalEvents.getInstance().dispatch(MynahEventNames.ROOT_RESIZE, { clientRect: this.rootBox });
        }
      });
      this.resizeObserver.observe(this.root);
    }
  }

  private readonly attachRootFocusListeners = (): void => {
    this.root?.setAttribute('tabindex', '0');
    this.root?.setAttribute('autofocus', 'true');
    this.root?.style.setProperty('outline', 'none');
    this.root?.addEventListener('focusin', this.onRootFocus, { capture: true });
    window.addEventListener('blur', this.onRootBlur);
  };

  private readonly onRootFocus = (e: FocusEvent): void => {
    if (!this.rootFocus) {
      this.rootFocus = true;
      MynahUIGlobalEvents.getInstance().dispatch(MynahEventNames.ROOT_FOCUS, { focusState: this.rootFocus });
    }
  };

  private readonly onRootBlur = (e: FocusEvent): void => {
    if (this.rootFocus) {
      this.rootFocus = false;
      MynahUIGlobalEvents.getInstance().dispatch(MynahEventNames.ROOT_FOCUS, { focusState: this.rootFocus });
    }
  };

  public readonly setFocusToRoot = (): void => {
    this.root?.focus();
  };

  public static getInstance (rootSelector?: string): DomBuilder {
    if (!DomBuilder.instance) {
      DomBuilder.instance = new DomBuilder(rootSelector != null ? rootSelector : 'body');
    }
    if (rootSelector != null) {
      DomBuilder.instance.setRoot(rootSelector);
    }
    return DomBuilder.instance;
  }

  setRoot = (rootSelector?: string): void => {
    this.resizeObserver.unobserve(this.root);
    this.root.removeEventListener('focus', this.onRootFocus);
    window.removeEventListener('blur', this.onRootBlur);
    this.root = this.extendDomFunctionality((DS(rootSelector ?? 'body')[0] ?? document.body) as HTMLElement);
    this.attachRootFocusListeners();
    this.resizeObserver.observe(this.root);
  };

  addClass = function (this: ExtendedHTMLElement, className: string): ExtendedHTMLElement {
    if (className !== '') {
      this.classList.add(className);
      // eslint-disable-next-line @typescript-eslint/prefer-includes
      if (this.builderObject?.classNames?.indexOf(className) === -1) {
        this.builderObject.classNames = [ ...this.builderObject.classNames, className ];
      }
    }
    return this;
  };

  removeClass = function (this: ExtendedHTMLElement, className: string): ExtendedHTMLElement {
    this.classList.remove(className);
    // eslint-disable-next-line @typescript-eslint/prefer-optional-chain
    if (this.builderObject.classNames !== undefined && this.builderObject.classNames.includes(className)) {
      this.builderObject.classNames.splice(this.builderObject.classNames.indexOf(className), 1);
    }
    return this;
  };

  toggleClass = function (this: ExtendedHTMLElement, className: string): ExtendedHTMLElement {
    if (this.classList.contains(className)) {
      this.removeClass(className);
    } else {
      this.addClass(className);
    }
    return this;
  };

  hasClass = function (this: ExtendedHTMLElement, className: string): boolean {
    return this.classList.contains(className);
  };

  insertChild = function (
    this: ExtendedHTMLElement,
    position: 'beforebegin' | 'afterbegin' | 'beforeend' | 'afterend',
    child: string | HTMLElement | ExtendedHTMLElement | Array<string | HTMLElement | ExtendedHTMLElement>
  ): ExtendedHTMLElement {
    // eslint-disable-next-line @typescript-eslint/strict-boolean-expressions
    if (child) {
      if (child instanceof Array) {
        child.forEach(childItem => {
          if (childItem instanceof Element) {
            this.insertAdjacentElement(position, childItem);
          } else if (typeof childItem === 'string') {
            this.insertAdjacentText(position, childItem);
          }
        });
      } else {
        if (child instanceof Element) {
          this.insertAdjacentElement(position, child);
        } else if (typeof child === 'string') {
          this.insertAdjacentText(position, child);
        }
      }
    }
    return this;
  };

  clearChildren = function (this: ExtendedHTMLElement, removePersistent: boolean): ExtendedHTMLElement {
    Array.from(this.childNodes).forEach((child: ExtendedHTMLElement | ChildNode) => {
      if (
        removePersistent ||
        // eslint-disable-next-line @typescript-eslint/strict-boolean-expressions
        !(child as ExtendedHTMLElement).builderObject ||
        (child as ExtendedHTMLElement).builderObject.persistent !== true
      ) {
        child.remove();
      }
    });
    return this;
  };

  extendDomFunctionality = function (this: DomBuilder, domElement: HTMLElement): ExtendedHTMLElement {
    const extendedElement: ExtendedHTMLElement = domElement as ExtendedHTMLElement;
    extendedElement.addClass = this.addClass.bind(extendedElement);
    extendedElement.removeClass = this.removeClass.bind(extendedElement);
    extendedElement.toggleClass = this.toggleClass.bind(extendedElement);
    extendedElement.hasClass = this.hasClass.bind(extendedElement);
    extendedElement.insertChild = this.insertChild.bind(extendedElement);
    extendedElement.clear = this.clearChildren.bind(extendedElement);
    return extendedElement;
  };

  build = (domBuilderObject: DomBuilderObject): ExtendedHTMLElement => {
    const readyToBuildObject: DomBuilderObject = { ...EmptyDomBuilderObject, ...domBuilderObject };
    const buildedDom = document.createElement(readyToBuildObject.type);

    // eslint-disable-next-line @typescript-eslint/prefer-nullish-coalescing
    buildedDom.classList.add(...(readyToBuildObject.classNames?.filter(className => className !== '') || []));

    (Object.keys(readyToBuildObject.events ?? {}) as Array<Partial<GenericEvents>>).forEach((eventName: GenericEvents) => {
      if (readyToBuildObject?.events !== undefined) {
        if (typeof readyToBuildObject?.events[eventName] === 'function') {
          buildedDom.addEventListener(eventName, readyToBuildObject.events[eventName] as (event?: any) => any);
        } else if (typeof readyToBuildObject?.events[eventName] === 'object') {
          buildedDom.addEventListener(
            eventName,
            (readyToBuildObject.events[eventName] as DomBuilderEventHandlerWithOptions).handler,
            (readyToBuildObject.events[eventName] as DomBuilderEventHandlerWithOptions).options ?? undefined
          );
        }
        if (eventName === 'dblclick' || eventName === 'click') {
          buildedDom.classList.add('mynah-ui-clickable-item');
        }
      }
    });

    Object.keys(readyToBuildObject.attributes ?? {}).forEach(attributeName =>
      buildedDom.setAttribute(attributeName, readyToBuildObject.attributes !== undefined ? readyToBuildObject.attributes[attributeName].toString() : '')
    );

    if (readyToBuildObject.testId != null && Config.getInstance().config.test) {
      buildedDom.setAttribute(testIds.selector, readyToBuildObject.testId);
    }

    if (typeof readyToBuildObject.innerHTML === 'string') {
      buildedDom.innerHTML = readyToBuildObject.innerHTML;
    } else if (readyToBuildObject.children !== undefined && readyToBuildObject.children?.length > 0) {
      this.insertChild.apply(buildedDom as ExtendedHTMLElement, [
        'beforeend',
        [
          ...readyToBuildObject.children.map((child: string | ExtendedHTMLElement | HTMLElement | DomBuilderObject) => {
            if (typeof child === 'string' || child instanceof HTMLElement) {
              return child;
            }
            return this.build(child);
          }),
        ],
      ]);
    }

    (buildedDom as ExtendedHTMLElement).builderObject = readyToBuildObject;
    (buildedDom as ExtendedHTMLElement).update = (builderObject: DomBuilderObjectFilled): ExtendedHTMLElement => {
      return this.update(buildedDom as ExtendedHTMLElement, builderObject);
    };
    this.extendDomFunctionality(buildedDom);
    return buildedDom as ExtendedHTMLElement;
  };

  update = function (domToUpdate: ExtendedHTMLElement, domBuilderObject: DomBuilderObjectFilled): ExtendedHTMLElement {
    if (domToUpdate.builderObject) {
      if (domBuilderObject.classNames !== undefined) {
        domToUpdate.classList.remove(...(domToUpdate.builderObject.classNames as string[]));
        domToUpdate.classList.add(...domBuilderObject.classNames.filter(className => className !== ''));
      }

      (Object.keys(domBuilderObject.events ?? {}) as Array<Partial<GenericEvents>>).forEach(eventName => {
        if (domToUpdate.builderObject.events !== undefined && domToUpdate.builderObject.events[eventName]) {
          domToUpdate.removeEventListener(
            eventName,
            (domToUpdate.builderObject.events[eventName] as DomBuilderEventHandlerWithOptions).handler ?? domToUpdate.builderObject.events[eventName]
          );
        }
        if (domBuilderObject.events !== undefined && domBuilderObject.events[eventName] !== undefined) {
          domToUpdate.addEventListener(eventName, domBuilderObject.events[eventName]);
        }
      });

      Object.keys(domBuilderObject.attributes ?? {}).forEach(attributeName => {
        if (domBuilderObject.attributes !== undefined && domBuilderObject.attributes[attributeName] === undefined) {
          domToUpdate.removeAttribute(attributeName);
        } else if (domBuilderObject.attributes !== undefined) {
          domToUpdate.setAttribute(attributeName, domBuilderObject.attributes[attributeName] as string);
        }
      });

      if (domBuilderObject.testId != null && Config.getInstance().config.test) {
        domToUpdate.setAttribute(testIds.selector, domBuilderObject.testId);
      }

      if (typeof domBuilderObject.innerHTML === 'string') {
        domToUpdate.innerHTML = domBuilderObject.innerHTML;
      } else if (domBuilderObject.children !== undefined && domBuilderObject.children.length > 0) {
        domToUpdate.clear();
        domToUpdate.insertChild('beforeend', domBuilderObject.children);
      }

      domToUpdate.builderObject = { ...EmptyDomBuilderObject, ...domBuilderObject } as DomBuilderObject;
    } else {
      console.warn('element was not created with dom builder');
    }
    return domToUpdate;
  };

  createPortal = (
    portalName: string,
    builderObject: DomBuilderObject,
    position: 'beforebegin' | 'afterbegin' | 'beforeend' | 'afterend'
  ): ExtendedHTMLElement => {
    const portalDom = this.build(builderObject);
    this.root.insertChild(position || 'beforeend', portalDom);
    this.portals[portalName] = portalDom;
    return portalDom;
  };

  getPortal = (portalName: string): ExtendedHTMLElement | undefined => this.portals[portalName] ?? undefined;
  removePortal = (portalName: string): void => this.portals[portalName]?.remove();
  removeAllPortals = (portalsWithName: MynahPortalNames): void => {
    Object.keys(this.portals).forEach(portalName => {
      if (portalName.match(portalsWithName) !== null) {
        this.portals[portalName].remove();
        // eslint-disable-next-line @typescript-eslint/no-dynamic-delete
        delete this.portals[portalName];
      }
    });
  };

  destroy = (): void => {
    DomBuilder.instance = undefined;
  };
}

export const htmlDecode = (input: string): string => {
  const e = document.createElement('textarea');
  e.innerHTML = input;
  return e.childNodes.length === 0 ? '' : e.childNodes[0].nodeValue ?? input;
};

export const getTypewriterPartsCss = (
  typewriterId: string,
  lastVisibleItemIndex: number,
  totalNumberOfItems: number,
  timeForEach: number): ExtendedHTMLElement =>
  DomBuilder.getInstance().build({
    type: 'style',
    attributes: {
      type: 'text/css'
    },
    persistent: true,
    innerHTML: `
  ${new Array(Math.max(0, totalNumberOfItems))
    .fill(null)
    .map((n, i) => {
      if (i < lastVisibleItemIndex) {
        return `
      .${typewriterId} .typewriter-part[index="${i}"] {
        opacity: 1 !important;
        animation: none;
      }
      `;
      }
      return `
      .${typewriterId} .typewriter-part[index="${i}"] {
        opacity: 0;
        animation: typewriter-reveal ${50 + timeForEach}ms linear forwards;
        animation-delay: ${(i - lastVisibleItemIndex) * timeForEach}ms;
      }
      `;
    })
    .join('')}
  `,
  });

export const cleanupElement = (elm: HTMLElement): void => {
  if (elm.querySelectorAll !== undefined) {
    Array.from(elm.querySelectorAll('*:empty:not(img, br, hr, input[type="checkbox"])')).forEach(emptyElement => {
      if (emptyElement.classList.length === 0) {
        emptyElement.remove();
      }
    });
  }
};
