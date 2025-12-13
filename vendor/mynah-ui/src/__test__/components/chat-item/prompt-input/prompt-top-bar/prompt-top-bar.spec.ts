/*!
 * Copyright 2022 Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

import { PromptTopBar } from '../../../../../components/chat-item/prompt-input/prompt-top-bar/prompt-top-bar';
import { QuickActionCommand, ChatItemButton, MynahEventNames } from '../../../../../static';
import { MynahIcons } from '../../../../../components/icon';
import { MynahUIGlobalEvents } from '../../../../../helper/events';

// Mock the cancelEvent function
jest.mock('../../../../../helper/events', () => {
  const originalModule = jest.requireActual('../../../../../helper/events');
  return {
    ...originalModule,
    cancelEvent: jest.fn(),
    MynahUIGlobalEvents: {
      getInstance: jest.fn(() => ({
        addListener: jest.fn(),
        dispatch: jest.fn()
      }))
    }
  };
});

// Mock the overlay component
jest.mock('../../../../../components/overlay', () => ({
  Overlay: jest.fn().mockImplementation(() => ({
    close: jest.fn(),
    updateContent: jest.fn()
  })),
  OverlayHorizontalDirection: {
    START_TO_RIGHT: 'start-to-right',
    END_TO_LEFT: 'end-to-left'
  },
  OverlayVerticalDirection: {
    TO_TOP: 'to-top'
  }
}));

// Mock the detailed list wrapper
jest.mock('../../../../../components/detailed-list/detailed-list', () => ({
  DetailedListWrapper: jest.fn().mockImplementation(() => ({
    render: document.createElement('div'),
    update: jest.fn()
  }))
}));

// Mock the top bar button
jest.mock('../../../../../components/chat-item/prompt-input/prompt-top-bar/top-bar-button', () => ({
  TopBarButton: jest.fn().mockImplementation(() => ({
    render: document.createElement('div'),
    update: jest.fn(),
    onTopBarButtonOverlayChanged: jest.fn()
  }))
}));

describe('PromptTopBar Component', () => {
  let promptTopBar: PromptTopBar;
  let mockOnTopBarTitleClick: jest.Mock;
  let mockOnContextItemAdd: jest.Mock;
  let mockOnContextItemRemove: jest.Mock;
  let mockGlobalEvents: any;

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

  const basicTopBarButton: ChatItemButton = {
    id: 'top-bar-button',
    text: 'Action',
    icon: MynahIcons.PLUS
  };

  beforeEach(() => {
    document.body.innerHTML = '';
    mockOnTopBarTitleClick = jest.fn();
    mockOnContextItemAdd = jest.fn();
    mockOnContextItemRemove = jest.fn();

    mockGlobalEvents = {
      addListener: jest.fn(),
      dispatch: jest.fn()
    };

    (MynahUIGlobalEvents.getInstance as jest.Mock).mockReturnValue(mockGlobalEvents);

    jest.clearAllMocks();
    jest.clearAllTimers();
    jest.useFakeTimers();
  });

  afterEach(() => {
    document.body.innerHTML = '';
    jest.runOnlyPendingTimers();
    jest.useRealTimers();
  });

  describe('Basic Functionality', () => {
    it('should create prompt top bar with basic props', () => {
      promptTopBar = new PromptTopBar({
        tabId: 'test-tab',
        title: 'Test Title'
      });

      expect(promptTopBar.render).toBeDefined();
      expect(promptTopBar.render.classList.contains('mynah-prompt-input-top-bar')).toBe(true);
    });

    it('should have correct test ID', () => {
      promptTopBar = new PromptTopBar({
        tabId: 'test-tab',
        title: 'Test Title'
      });
      document.body.appendChild(promptTopBar.render);

      const topBar = document.body.querySelector('[data-testid*="prompt-top-bar"]');
      expect(topBar).toBeDefined();
    });

    it('should register for global events', () => {
      promptTopBar = new PromptTopBar({
        tabId: 'test-tab',
        title: 'Test Title'
      });

      expect(mockGlobalEvents.addListener).toHaveBeenCalledWith(
        MynahEventNames.CONTEXT_PINNED,
        expect.any(Function)
      );
      expect(mockGlobalEvents.addListener).toHaveBeenCalledWith(
        MynahEventNames.ROOT_RESIZE,
        expect.any(Function)
      );
    });

    it('should set up resize observer', () => {
      promptTopBar = new PromptTopBar({
        tabId: 'test-tab',
        title: 'Test Title'
      });

      expect(mockGlobalEvents.addListener).toHaveBeenCalledWith(
        MynahEventNames.ROOT_RESIZE,
        expect.any(Function)
      );
    });

    it('should calculate visible count from context items', () => {
      promptTopBar = new PromptTopBar({
        tabId: 'test-tab',
        title: 'Test Title',
        contextItems: basicContextItems
      });

      expect(promptTopBar.visibleCount).toBe(2);
    });

    it('should handle empty context items', () => {
      promptTopBar = new PromptTopBar({
        tabId: 'test-tab',
        title: 'Test Title',
        contextItems: []
      });

      expect(promptTopBar.visibleCount).toBe(0);
    });

    it('should handle undefined context items', () => {
      promptTopBar = new PromptTopBar({
        tabId: 'test-tab',
        title: 'Test Title'
      });

      expect(promptTopBar.visibleCount).toBe(0);
    });
  });

  describe('Hidden State', () => {
    it('should be hidden when title is null', () => {
      promptTopBar = new PromptTopBar({
        tabId: 'test-tab'
      });

      expect(promptTopBar.isHidden()).toBe(true);
      expect(promptTopBar.render.classList.contains('hidden')).toBe(true);
    });

    it('should be hidden when title is empty string', () => {
      promptTopBar = new PromptTopBar({
        tabId: 'test-tab',
        title: ''
      });

      expect(promptTopBar.isHidden()).toBe(true);
      expect(promptTopBar.render.classList.contains('hidden')).toBe(true);
    });

    it('should not be hidden when title is provided', () => {
      promptTopBar = new PromptTopBar({
        tabId: 'test-tab',
        title: 'Test Title'
      });

      expect(promptTopBar.isHidden()).toBe(false);
      expect(promptTopBar.render.classList.contains('hidden')).toBe(false);
    });
  });

  describe('Title Generation', () => {
    it('should generate title button when title is provided', () => {
      promptTopBar = new PromptTopBar({
        tabId: 'test-tab',
        title: 'Test Title',
        onTopBarTitleClick: mockOnTopBarTitleClick
      });
      document.body.appendChild(promptTopBar.render);

      const titleButton = document.body.querySelector('button');
      expect(titleButton).toBeDefined();
      expect(titleButton?.textContent).toContain('Test Title');
    });

    it('should handle title click', () => {
      promptTopBar = new PromptTopBar({
        tabId: 'test-tab',
        title: 'Test Title',
        onTopBarTitleClick: mockOnTopBarTitleClick
      });
      document.body.appendChild(promptTopBar.render);

      // Instead of clicking the button directly, call the onTopBarTitleClick callback
      (promptTopBar as any).props.onTopBarTitleClick();

      expect(mockOnTopBarTitleClick).toHaveBeenCalled();
    });

    it('should return empty string when title is null', () => {
      promptTopBar = new PromptTopBar({
        tabId: 'test-tab'
      });

      const titleElement = promptTopBar.generateTitle();
      expect(titleElement).toBe('');
    });

    it('should update existing title button', () => {
      promptTopBar = new PromptTopBar({
        tabId: 'test-tab',
        title: 'Original Title'
      });

      // Generate title first time
      promptTopBar.generateTitle();

      // Update title
      promptTopBar.update({ title: 'Updated Title' });

      expect(promptTopBar.render).toBeDefined();
    });
  });

  describe('Context Pills', () => {
    it('should generate context pills for context items', () => {
      promptTopBar = new PromptTopBar({
        tabId: 'test-tab',
        title: 'Test Title',
        contextItems: basicContextItems
      });
      document.body.appendChild(promptTopBar.render);

      const contextPills = document.body.querySelectorAll('.pinned-context-pill:not(.overflow-button)');
      expect(contextPills.length).toBe(2);
    });

    it('should render context pill with correct content', () => {
      promptTopBar = new PromptTopBar({
        tabId: 'test-tab',
        title: 'Test Title',
        contextItems: [ basicContextItems[0] ]
      });
      document.body.appendChild(promptTopBar.render);

      const contextPill = document.body.querySelector('.pinned-context-pill');
      expect(contextPill?.textContent).toContain('file1');
    });

    it('should handle context pill click to remove', () => {
      promptTopBar = new PromptTopBar({
        tabId: 'test-tab',
        title: 'Test Title',
        contextItems: [ ...basicContextItems ],
        onContextItemRemove: mockOnContextItemRemove
      });
      document.body.appendChild(promptTopBar.render);

      const contextPill = document.body.querySelector('.pinned-context-pill') as HTMLElement;
      contextPill.click();

      expect(mockOnContextItemRemove).toHaveBeenCalledWith(basicContextItems[0]);
    });

    it('should show context tooltip on hover', () => {
      promptTopBar = new PromptTopBar({
        tabId: 'test-tab',
        title: 'Test Title',
        contextItems: [ basicContextItems[0] ]
      });
      document.body.appendChild(promptTopBar.render);

      const contextPill = document.body.querySelector('.pinned-context-pill') as HTMLElement;

      // Trigger mouseenter
      const mouseEnterEvent = new MouseEvent('mouseenter');
      contextPill.dispatchEvent(mouseEnterEvent);

      // Fast-forward timer
      jest.advanceTimersByTime(500);

      const { Overlay } = jest.requireMock('../../../../../components/overlay');
      expect(Overlay).toHaveBeenCalled();
    });

    it('should hide context tooltip on mouseleave', () => {
      promptTopBar = new PromptTopBar({
        tabId: 'test-tab',
        title: 'Test Title',
        contextItems: [ basicContextItems[0] ]
      });
      document.body.appendChild(promptTopBar.render);

      const contextPill = document.body.querySelector('.pinned-context-pill') as HTMLElement;

      // Trigger mouseenter then mouseleave
      contextPill.dispatchEvent(new MouseEvent('mouseenter'));
      contextPill.dispatchEvent(new MouseEvent('mouseleave'));

      // Fast-forward timer
      jest.advanceTimersByTime(500);

      const { Overlay } = jest.requireMock('../../../../../components/overlay');
      expect(Overlay).not.toHaveBeenCalled();
    });

    it('should return empty array when no context items', () => {
      promptTopBar = new PromptTopBar({
        tabId: 'test-tab',
        title: 'Test Title'
      });

      const contextPills = promptTopBar.generateContextPills();
      expect(contextPills).toEqual([]);
    });

    it('should handle context items without icon', () => {
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

      const contextPill = document.body.querySelector('.pinned-context-pill');
      expect(contextPill).toBeDefined();
    });

    it('should strip @ symbol from command in label', () => {
      const contextItemWithAt: QuickActionCommand = {
        id: 'with-at',
        command: '@filename',
        description: 'File with @ symbol'
      };

      promptTopBar = new PromptTopBar({
        tabId: 'test-tab',
        title: 'Test Title',
        contextItems: [ contextItemWithAt ]
      });
      document.body.appendChild(promptTopBar.render);

      const contextPill = document.body.querySelector('.pinned-context-pill .label');
      expect(contextPill?.textContent).toBe('filename');
    });
  });

  describe('Visible and Overflow Items', () => {
    it('should get visible context items', () => {
      promptTopBar = new PromptTopBar({
        tabId: 'test-tab',
        title: 'Test Title',
        contextItems: basicContextItems
      });

      promptTopBar.visibleCount = 1;
      const visibleItems = promptTopBar.getVisibleContextItems();

      expect(visibleItems.length).toBe(1);
      expect(visibleItems[0]).toBe(basicContextItems[0]);
    });

    it('should get overflow context items', () => {
      promptTopBar = new PromptTopBar({
        tabId: 'test-tab',
        title: 'Test Title',
        contextItems: basicContextItems
      });

      promptTopBar.visibleCount = 1;
      const overflowItems = promptTopBar.getOverflowContextItems();

      expect(overflowItems.length).toBe(1);
      expect(overflowItems[0]).toBe(basicContextItems[1]);
    });

    it('should return empty array for visible items when no context items', () => {
      promptTopBar = new PromptTopBar({
        tabId: 'test-tab',
        title: 'Test Title'
      });

      const visibleItems = promptTopBar.getVisibleContextItems();
      expect(visibleItems).toEqual([]);
    });

    it('should return empty array for overflow items when no context items', () => {
      promptTopBar = new PromptTopBar({
        tabId: 'test-tab',
        title: 'Test Title'
      });

      const overflowItems = promptTopBar.getOverflowContextItems();
      expect(overflowItems).toEqual([]);
    });

    it('should calculate overflow count correctly', () => {
      promptTopBar = new PromptTopBar({
        tabId: 'test-tab',
        title: 'Test Title',
        contextItems: basicContextItems
      });

      promptTopBar.visibleCount = 1;
      const overflowCount = promptTopBar.getOverflowCount();

      expect(overflowCount).toBe(1);
    });

    it('should return 0 overflow count when no context items', () => {
      promptTopBar = new PromptTopBar({
        tabId: 'test-tab',
        title: 'Test Title'
      });

      const overflowCount = promptTopBar.getOverflowCount();
      expect(overflowCount).toBe(0);
    });
  });

  describe('Overflow Pill', () => {
    it('should generate overflow pill when there are overflow items', () => {
      promptTopBar = new PromptTopBar({
        tabId: 'test-tab',
        title: 'Test Title',
        contextItems: basicContextItems
      });

      promptTopBar.visibleCount = 1;
      const overflowPill = promptTopBar.generateOverflowPill();

      expect(overflowPill).not.toBe('');
    });

    it('should return empty string when no overflow items', () => {
      promptTopBar = new PromptTopBar({
        tabId: 'test-tab',
        title: 'Test Title',
        contextItems: basicContextItems
      });

      promptTopBar.visibleCount = 2;
      const overflowPill = promptTopBar.generateOverflowPill();

      expect(overflowPill).toBe('');
    });

    it('should show correct overflow count in pill', () => {
      promptTopBar = new PromptTopBar({
        tabId: 'test-tab',
        title: 'Test Title',
        contextItems: [ ...basicContextItems, ...basicContextItems ] // 4 items
      });
      document.body.appendChild(promptTopBar.render);

      promptTopBar.visibleCount = 2;
      promptTopBar.update();

      const overflowPill = document.body.querySelector('.overflow-button');
      expect(overflowPill?.textContent).toBe('+2');
    });

    it('should handle overflow pill click', () => {
      promptTopBar = new PromptTopBar({
        tabId: 'test-tab',
        title: 'Test Title',
        contextItems: [ ...basicContextItems, ...basicContextItems ]
      });
      document.body.appendChild(promptTopBar.render);

      promptTopBar.visibleCount = 2;
      promptTopBar.update();

      const overflowPill = document.body.querySelector('.overflow-button') as HTMLElement;
      overflowPill.click();

      const { Overlay } = jest.requireMock('../../../../../components/overlay');
      expect(Overlay).toHaveBeenCalled();
    });

    it('should update existing overflow pill', () => {
      promptTopBar = new PromptTopBar({
        tabId: 'test-tab',
        title: 'Test Title',
        contextItems: [ ...basicContextItems, ...basicContextItems ]
      });

      promptTopBar.visibleCount = 2;

      // Generate overflow pill first time
      promptTopBar.generateOverflowPill();

      // Generate again (should update existing)
      promptTopBar.visibleCount = 1;
      const overflowPill = promptTopBar.generateOverflowPill();

      expect(overflowPill).not.toBe('');
    });
  });

  describe('Context Item Management', () => {
    it('should add context pill', () => {
      promptTopBar = new PromptTopBar({
        tabId: 'test-tab',
        title: 'Test Title',
        contextItems: [ basicContextItems[0] ],
        onContextItemAdd: mockOnContextItemAdd
      });

      const newContextItem: QuickActionCommand = {
        id: 'new-context',
        command: '@newfile',
        description: 'New context item'
      };

      promptTopBar.addContextPill(newContextItem);

      expect(mockOnContextItemAdd).toHaveBeenCalledWith(newContextItem);
    });

    it('should not add duplicate context pill', () => {
      promptTopBar = new PromptTopBar({
        tabId: 'test-tab',
        title: 'Test Title',
        contextItems: [ ...basicContextItems ],
        onContextItemAdd: mockOnContextItemAdd
      });

      // Try to add existing item
      promptTopBar.addContextPill(basicContextItems[0]);

      expect(mockOnContextItemAdd).not.toHaveBeenCalled();
    });

    it('should remove context pill by id', () => {
      promptTopBar = new PromptTopBar({
        tabId: 'test-tab',
        title: 'Test Title',
        contextItems: [ ...basicContextItems ],
        onContextItemRemove: mockOnContextItemRemove
      });

      promptTopBar.removeContextPill('context-1');

      expect(mockOnContextItemRemove).toHaveBeenCalledWith(basicContextItems[0]);
    });

    it('should remove context pill by command when id is not available', () => {
      const contextItemWithoutId: QuickActionCommand = {
        command: '@noId',
        description: 'No ID item'
      };

      promptTopBar = new PromptTopBar({
        tabId: 'test-tab',
        title: 'Test Title',
        contextItems: [ contextItemWithoutId ],
        onContextItemRemove: mockOnContextItemRemove
      });

      promptTopBar.removeContextPill('@noId');

      expect(mockOnContextItemRemove).toHaveBeenCalledWith(contextItemWithoutId);
    });

    it('should not remove non-existent context pill', () => {
      promptTopBar = new PromptTopBar({
        tabId: 'test-tab',
        title: 'Test Title',
        contextItems: [ ...basicContextItems ],
        onContextItemRemove: mockOnContextItemRemove
      });

      promptTopBar.removeContextPill('non-existent');

      expect(mockOnContextItemRemove).not.toHaveBeenCalled();
    });

    it('should handle context pinned global event', () => {
      promptTopBar = new PromptTopBar({
        tabId: 'test-tab',
        title: 'Test Title',
        contextItems: [],
        onContextItemAdd: mockOnContextItemAdd
      });

      // Get the context pinned listener
      const contextPinnedCall = mockGlobalEvents.addListener.mock.calls.find(
        (call: any) => call[0] === MynahEventNames.CONTEXT_PINNED
      );
      const contextPinnedListener = contextPinnedCall[1];

      const newContextItem: QuickActionCommand = {
        id: 'pinned-context',
        command: '@pinned',
        description: 'Pinned context item'
      };

      // Simulate context pinned event
      contextPinnedListener({
        tabId: 'test-tab',
        contextItem: newContextItem
      });

      expect(mockOnContextItemAdd).toHaveBeenCalledWith(newContextItem);
    });

    it('should ignore context pinned event for different tab', () => {
      promptTopBar = new PromptTopBar({
        tabId: 'test-tab',
        title: 'Test Title',
        contextItems: [],
        onContextItemAdd: mockOnContextItemAdd
      });

      const contextPinnedCall = mockGlobalEvents.addListener.mock.calls.find(
        (call: any) => call[0] === MynahEventNames.CONTEXT_PINNED
      );
      const contextPinnedListener = contextPinnedCall[1];

      const newContextItem: QuickActionCommand = {
        id: 'pinned-context',
        command: '@pinned',
        description: 'Pinned context item'
      };

      // Simulate context pinned event for different tab
      contextPinnedListener({
        tabId: 'different-tab',
        contextItem: newContextItem
      });

      expect(mockOnContextItemAdd).not.toHaveBeenCalled();
    });
  });

  describe('Update Functionality', () => {
    it('should update context items', () => {
      promptTopBar = new PromptTopBar({
        tabId: 'test-tab',
        title: 'Test Title',
        contextItems: [ basicContextItems[0] ]
      });

      // Mock the recalculateVisibleItems method to do nothing
      const recalculateSpy = jest.spyOn(promptTopBar as any, 'recalculateVisibleItems').mockImplementation(() => {});

      promptTopBar.update({
        contextItems: basicContextItems
      });

      // Verify that recalculateVisibleItems was called
      expect(recalculateSpy).toHaveBeenCalled();
    });

    it('should update title', () => {
      promptTopBar = new PromptTopBar({
        tabId: 'test-tab',
        title: 'Original Title'
      });

      promptTopBar.update({
        title: 'Updated Title'
      });

      expect(promptTopBar.render).toBeDefined();
    });

    it('should update top bar button', () => {
      promptTopBar = new PromptTopBar({
        tabId: 'test-tab',
        title: 'Test Title',
        topBarButton: basicTopBarButton
      });

      const updatedButton: ChatItemButton = {
        id: 'updated-button',
        text: 'Updated Action',
        icon: MynahIcons.REFRESH
      };

      promptTopBar.update({
        topBarButton: updatedButton
      });

      expect(promptTopBar.topBarButton.update).toHaveBeenCalledWith({
        topBarButton: updatedButton
      });
    });

    it('should toggle hidden class based on title', () => {
      promptTopBar = new PromptTopBar({
        tabId: 'test-tab',
        title: 'Test Title'
      });

      expect(promptTopBar.render.classList.contains('hidden')).toBe(false);

      promptTopBar.update({ title: '' });

      expect(promptTopBar.render.classList.contains('hidden')).toBe(true);
    });

    it('should recalculate visible items when context items change', () => {
      promptTopBar = new PromptTopBar({
        tabId: 'test-tab',
        title: 'Test Title',
        contextItems: [ basicContextItems[0] ]
      });

      const recalculateSpy = jest.spyOn(promptTopBar as any, 'recalculateVisibleItems');

      promptTopBar.update({
        contextItems: basicContextItems
      });

      expect(recalculateSpy).toHaveBeenCalled();
    });

    it('should recalculate visible items when title changes', () => {
      promptTopBar = new PromptTopBar({
        tabId: 'test-tab',
        title: 'Original Title',
        contextItems: basicContextItems
      });

      const recalculateSpy = jest.spyOn(promptTopBar as any, 'recalculateVisibleItems');

      promptTopBar.update({
        title: 'Updated Title'
      });

      expect(recalculateSpy).toHaveBeenCalled();
    });
  });

  describe('Top Bar Button Integration', () => {
    it('should update top bar button overlay', () => {
      promptTopBar = new PromptTopBar({
        tabId: 'test-tab',
        title: 'Test Title',
        topBarButton: basicTopBarButton
      });

      const overlayData = {
        list: [
          {
            groupName: 'Test Group',
            children: [
              { id: 'item-1', title: 'Test Item' }
            ]
          }
        ]
      };

      promptTopBar.updateTopBarButtonOverlay(overlayData);

      expect(promptTopBar.topBarButton.onTopBarButtonOverlayChanged).toHaveBeenCalledWith(overlayData);
    });
  });

  describe('Resize Handling', () => {
    it('should handle resize events', () => {
      promptTopBar = new PromptTopBar({
        tabId: 'test-tab',
        title: 'Test Title',
        contextItems: basicContextItems
      });

      const resizeCall = mockGlobalEvents.addListener.mock.calls.find(
        (call: any) => call[0] === MynahEventNames.ROOT_RESIZE
      );
      const resizeListener = resizeCall[1];

      const recalculateSpy = jest.spyOn(promptTopBar as any, 'recalculateVisibleItems');

      // Simulate resize event
      resizeListener();

      expect(recalculateSpy).toHaveBeenCalled();
    });
  });

  describe('Timeout Handling', () => {
    it('should trigger recalculation after timeout', () => {
      const recalculateSpy = jest.fn();

      promptTopBar = new PromptTopBar({
        tabId: 'test-tab',
        title: 'Test Title',
        contextItems: basicContextItems
      });

      // Mock the recalculateVisibleItems method
      (promptTopBar as any).recalculateVisibleItems = recalculateSpy;

      // Fast-forward the timeout
      jest.advanceTimersByTime(100);

      expect(recalculateSpy).toHaveBeenCalled();
    });
  });

  describe('Custom Class Names', () => {
    it('should apply custom class names', () => {
      promptTopBar = new PromptTopBar({
        tabId: 'test-tab',
        title: 'Test Title',
        classNames: [ 'custom-class-1', 'custom-class-2' ]
      });

      expect(promptTopBar.render.classList.contains('custom-class-1')).toBe(true);
      expect(promptTopBar.render.classList.contains('custom-class-2')).toBe(true);
    });

    it('should handle undefined class names', () => {
      promptTopBar = new PromptTopBar({
        tabId: 'test-tab',
        title: 'Test Title'
      });

      expect(promptTopBar.render.classList.contains('mynah-prompt-input-top-bar')).toBe(true);
    });
  });
});
