/*!
 * Copyright 2022 Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

import { PromptTopBar } from '../../../../../components/chat-item/prompt-input/prompt-top-bar/prompt-top-bar';
import { QuickActionCommand } from '../../../../../static';
import { MynahIcons } from '../../../../../components/icon';

// Mock the global events
jest.mock('../../../../../helper/events', () => ({
  MynahUIGlobalEvents: {
    getInstance: jest.fn(() => ({
      addListener: jest.fn(),
      dispatch: jest.fn()
    }))
  },
  cancelEvent: jest.fn()
}));

// Mock the overlay component
jest.mock('../../../../../components/overlay', () => {
  const mockClose = jest.fn();
  const mockUpdateContent = jest.fn();
  const mockOverlay = jest.fn().mockImplementation(() => ({
    close: mockClose,
    updateContent: mockUpdateContent
  }));

  return {
    Overlay: mockOverlay,
    OverlayHorizontalDirection: {
      START_TO_RIGHT: 'start-to-right',
      END_TO_LEFT: 'end-to-left'
    },
    OverlayVerticalDirection: {
      TO_TOP: 'to-top'
    }
  };
});

describe('PromptTopBar Edge Cases', () => {
  let promptTopBar: PromptTopBar;

  const basicContextItems: QuickActionCommand[] = [
    {
      id: 'context-1',
      command: '@file1',
      description: 'First context item',
      icon: MynahIcons.FILE
    },
    {
      id: 'context-2',
      command: '@file2',
      description: 'Second context item',
      icon: MynahIcons.FILE
    }
  ];

  beforeEach(() => {
    document.body.innerHTML = '';
    jest.clearAllMocks();
    jest.useFakeTimers();
  });

  afterEach(() => {
    document.body.innerHTML = '';
    jest.clearAllMocks();
    jest.useRealTimers();
  });

  describe('Context Tooltip', () => {
    it('should show tooltip with icon when context item has icon', () => {
      promptTopBar = new PromptTopBar({
        tabId: 'test-tab',
        title: 'Test Title',
        contextItems: [ basicContextItems[0] ]
      });
      document.body.appendChild(promptTopBar.render);

      const contextPill = document.body.querySelector('.pinned-context-pill') as HTMLElement;

      // Create a proper mouse event with target
      const mouseEvent = new MouseEvent('mouseenter');
      Object.defineProperty(mouseEvent, 'target', {
        value: contextPill,
        enumerable: true
      });

      (promptTopBar as any).showContextTooltip(mouseEvent, basicContextItems[0]);

      // Fast-forward timer
      jest.advanceTimersByTime(500);

      const { Overlay } = jest.requireMock('../../../../../components/overlay');
      expect(Overlay).toHaveBeenCalled();
    });

    it('should show tooltip without icon when context item has no icon', () => {
      const contextItemWithoutIcon: QuickActionCommand = {
        id: 'no-icon',
        command: '@noicon',
        description: 'No icon item'
      };

      promptTopBar = new PromptTopBar({
        tabId: 'test-tab',
        title: 'Test Title',
        contextItems: [ contextItemWithoutIcon ]
      });
      document.body.appendChild(promptTopBar.render);

      const contextPill = document.body.querySelector('.pinned-context-pill') as HTMLElement;

      const mouseEvent = new MouseEvent('mouseenter');
      Object.defineProperty(mouseEvent, 'target', {
        value: contextPill,
        enumerable: true
      });

      (promptTopBar as any).showContextTooltip(mouseEvent, contextItemWithoutIcon);

      jest.advanceTimersByTime(500);

      const { Overlay } = jest.requireMock('../../../../../components/overlay');
      expect(Overlay).toHaveBeenCalled();
    });

    it('should show tooltip without description when context item has no description', () => {
      const contextItemWithoutDescription: QuickActionCommand = {
        id: 'no-desc',
        command: '@nodesc',
        icon: MynahIcons.FILE
      };

      promptTopBar = new PromptTopBar({
        tabId: 'test-tab',
        title: 'Test Title',
        contextItems: [ contextItemWithoutDescription ]
      });
      document.body.appendChild(promptTopBar.render);

      const contextPill = document.body.querySelector('.pinned-context-pill') as HTMLElement;

      const mouseEvent = new MouseEvent('mouseenter');
      Object.defineProperty(mouseEvent, 'target', {
        value: contextPill,
        enumerable: true
      });

      (promptTopBar as any).showContextTooltip(mouseEvent, contextItemWithoutDescription);

      jest.advanceTimersByTime(500);

      const { Overlay } = jest.requireMock('../../../../../components/overlay');
      expect(Overlay).toHaveBeenCalled();
    });

    it('should clear existing tooltip timeout when showing new tooltip', () => {
      promptTopBar = new PromptTopBar({
        tabId: 'test-tab',
        title: 'Test Title',
        contextItems: [ basicContextItems[0] ]
      });
      document.body.appendChild(promptTopBar.render);

      const contextPill = document.body.querySelector('.pinned-context-pill') as HTMLElement;

      const mouseEvent = new MouseEvent('mouseenter');
      Object.defineProperty(mouseEvent, 'target', {
        value: contextPill,
        enumerable: true
      });

      // Set up a tooltip timeout
      (promptTopBar as any).contextTooltipTimeout = setTimeout(() => {}, 1000);
      const clearTimeoutSpy = jest.spyOn(global, 'clearTimeout');

      // Show tooltip
      (promptTopBar as any).showContextTooltip(mouseEvent, basicContextItems[0]);

      expect(clearTimeoutSpy).toHaveBeenCalled();
    });

    it('should hide tooltip and clear timeout', () => {
      promptTopBar = new PromptTopBar({
        tabId: 'test-tab',
        title: 'Test Title',
        contextItems: [ basicContextItems[0] ]
      });

      // Set up a tooltip timeout
      (promptTopBar as any).contextTooltipTimeout = setTimeout(() => {}, 1000);
      const clearTimeoutSpy = jest.spyOn(global, 'clearTimeout');

      // Create a mock tooltip
      const mockTooltip = { close: jest.fn() };
      (promptTopBar as any).contextTooltip = mockTooltip;

      (promptTopBar as any).hideContextTooltip();

      expect(clearTimeoutSpy).toHaveBeenCalled();
      expect(mockTooltip.close).toHaveBeenCalled();
      expect((promptTopBar as any).contextTooltip).toBeNull();
    });
  });

  describe('Responsive Behavior', () => {
    beforeEach(() => {
      // Mock DOM measurements
      Object.defineProperty(HTMLElement.prototype, 'offsetWidth', {
        configurable: true,
        value: 100
      });
    });

    it('should recalculate visible items based on container width', () => {
      promptTopBar = new PromptTopBar({
        tabId: 'test-tab',
        title: 'Test Title',
        contextItems: basicContextItems
      });
      document.body.appendChild(promptTopBar.render);

      // Mock container width
      Object.defineProperty(promptTopBar.render, 'offsetWidth', {
        value: 800
      });

      // Mock querySelectorAll to return mock elements
      const mockPills = Array.from({ length: 2 }, () => ({
        offsetWidth: 80
      }));

      jest.spyOn(promptTopBar.render, 'querySelectorAll').mockReturnValue(mockPills as any);

      // Spy on the update method
      const updateSpy = jest.spyOn(promptTopBar, 'update');

      (promptTopBar as any).recalculateVisibleItems();

      // Should not call update if visibleCount doesn't change
      expect(updateSpy).not.toHaveBeenCalled();
    });

    it('should handle width increase scenario', () => {
      promptTopBar = new PromptTopBar({
        tabId: 'test-tab',
        title: 'Test Title',
        contextItems: basicContextItems
      });
      document.body.appendChild(promptTopBar.render);

      // Set initial visible count with overflow
      promptTopBar.visibleCount = 1;

      // Mock container with large width
      Object.defineProperty(promptTopBar.render, 'offsetWidth', {
        value: 1200
      });

      // Mock overflow button
      const mockOverflowButton = { offsetWidth: 50 };
      Object.defineProperty(promptTopBar, 'overflowButton', {
        value: mockOverflowButton
      });

      // Mock querySelectorAll to return mock elements
      const mockPills = Array.from({ length: 1 }, () => ({
        offsetWidth: 80
      }));

      jest.spyOn(promptTopBar.render, 'querySelectorAll').mockReturnValue(mockPills as any);

      // Spy on the update method
      const updateSpy = jest.spyOn(promptTopBar, 'update');

      (promptTopBar as any).recalculateVisibleItems();

      // Should call update when visibleCount changes
      expect(updateSpy).toHaveBeenCalled();
    });

    it('should handle width decrease scenario', () => {
      promptTopBar = new PromptTopBar({
        tabId: 'test-tab',
        title: 'Test Title',
        contextItems: basicContextItems
      });
      document.body.appendChild(promptTopBar.render);

      // Mock container with small width
      Object.defineProperty(promptTopBar.render, 'offsetWidth', {
        value: 300
      });

      // Mock querySelectorAll to return mock elements
      const mockPills = Array.from({ length: 2 }, () => ({
        offsetWidth: 200
      }));

      jest.spyOn(promptTopBar.render, 'querySelectorAll').mockReturnValue(mockPills as any);

      // Spy on the update method
      const updateSpy = jest.spyOn(promptTopBar, 'update');

      (promptTopBar as any).recalculateVisibleItems();

      // Should call update when visibleCount changes
      expect(updateSpy).toHaveBeenCalled();
    });

    it('should return early when no context items', () => {
      promptTopBar = new PromptTopBar({
        tabId: 'test-tab',
        title: 'Test Title',
        contextItems: []
      });

      const updateSpy = jest.spyOn(promptTopBar, 'update');

      (promptTopBar as any).recalculateVisibleItems();

      expect(updateSpy).not.toHaveBeenCalled();
    });
  });
});
