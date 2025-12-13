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
const mockUpdateContent = jest.fn();
const mockClose = jest.fn();

jest.mock('../../../../../components/overlay', () => ({
  Overlay: jest.fn().mockImplementation(() => ({
    close: mockClose,
    updateContent: mockUpdateContent
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
const mockDetailedListUpdate = jest.fn();

jest.mock('../../../../../components/detailed-list/detailed-list', () => ({
  DetailedListWrapper: jest.fn().mockImplementation(() => ({
    render: document.createElement('div'),
    update: mockDetailedListUpdate
  }))
}));

describe('PromptTopBar Overflow Detailed Tests', () => {
  let promptTopBar: PromptTopBar;
  let mockOnContextItemRemove: jest.Mock;

  const manyContextItems: QuickActionCommand[] = Array.from({ length: 10 }, (_, i) => ({
    id: `context-${i}`,
    command: `@file${i}`,
    description: `Context item ${i}`,
    icon: MynahIcons.FILE
  }));

  beforeEach(() => {
    document.body.innerHTML = '';
    mockOnContextItemRemove = jest.fn();
    jest.clearAllMocks();
  });

  afterEach(() => {
    document.body.innerHTML = '';
    jest.clearAllMocks();
  });

  describe('Overflow Overlay Generation', () => {
    it('should generate overflow overlay children', () => {
      promptTopBar = new PromptTopBar({
        tabId: 'test-tab',
        title: 'Test Title',
        contextItems: manyContextItems,
        onContextItemRemove: mockOnContextItemRemove
      });

      // Set visible count to create overflow
      promptTopBar.visibleCount = 5;

      const overlayChildren = promptTopBar.generateOverflowOverlayChildren();

      expect(overlayChildren).toBeDefined();
      expect(overlayChildren.classList.contains('mynah-chat-prompt-quick-picks-overlay-wrapper')).toBe(true);
    });

    it('should create detailed list wrapper on first call', () => {
      promptTopBar = new PromptTopBar({
        tabId: 'test-tab',
        title: 'Test Title',
        contextItems: manyContextItems,
        onContextItemRemove: mockOnContextItemRemove
      });

      promptTopBar.visibleCount = 5;

      // First call to create the wrapper
      promptTopBar.generateOverflowOverlayChildren();

      const { DetailedListWrapper } = jest.requireMock('../../../../../components/detailed-list/detailed-list');
      expect(DetailedListWrapper).toHaveBeenCalled();
    });

    it('should update detailed list wrapper on subsequent calls', () => {
      promptTopBar = new PromptTopBar({
        tabId: 'test-tab',
        title: 'Test Title',
        contextItems: manyContextItems,
        onContextItemRemove: mockOnContextItemRemove
      });

      promptTopBar.visibleCount = 5;

      // First call to create the wrapper
      promptTopBar.generateOverflowOverlayChildren();

      // Set up the overflowListContainer property manually for testing
      (promptTopBar as any).overflowListContainer = {
        update: mockDetailedListUpdate
      };

      // Change visible count to update overflow items
      promptTopBar.visibleCount = 3;

      // Call again to trigger update path
      promptTopBar.generateOverflowOverlayChildren();

      expect(mockDetailedListUpdate).toHaveBeenCalled();
    });

    it('should close overlay when no overflow items remain', () => {
      promptTopBar = new PromptTopBar({
        tabId: 'test-tab',
        title: 'Test Title',
        contextItems: manyContextItems
      });

      promptTopBar.visibleCount = 5;

      // Create overlay first
      promptTopBar.showOverflowOverlay(new Event('click'));

      // Set up the overflowOverlay property manually for testing
      (promptTopBar as any).overflowOverlay = { close: mockClose };

      // Set up the overflowListContainer property manually for testing
      (promptTopBar as any).overflowListContainer = {
        update: mockDetailedListUpdate
      };

      // Set visible count to show all items (no overflow)
      promptTopBar.visibleCount = 10;

      // Generate overlay children again
      promptTopBar.generateOverflowOverlayChildren();

      expect(mockClose).toHaveBeenCalled();
    });
  });

  describe('Overflow Item Handling', () => {
    it('should handle item click in overflow list', () => {
      promptTopBar = new PromptTopBar({
        tabId: 'test-tab',
        title: 'Test Title',
        contextItems: manyContextItems,
        onContextItemRemove: mockOnContextItemRemove
      });

      promptTopBar.visibleCount = 5;
      promptTopBar.generateOverflowOverlayChildren();

      const { DetailedListWrapper } = jest.requireMock('../../../../../components/detailed-list/detailed-list');
      const detailedListCall = DetailedListWrapper.mock.calls[0];
      const detailedListProps = detailedListCall[0];

      // Simulate item click
      const testItem = { id: 'context-5', title: '@file5' };
      detailedListProps.onItemClick(testItem);

      expect(mockOnContextItemRemove).toHaveBeenCalled();
    });

    it('should handle item action click in overflow list', () => {
      promptTopBar = new PromptTopBar({
        tabId: 'test-tab',
        title: 'Test Title',
        contextItems: manyContextItems,
        onContextItemRemove: mockOnContextItemRemove
      });

      promptTopBar.visibleCount = 5;
      promptTopBar.generateOverflowOverlayChildren();

      const { DetailedListWrapper } = jest.requireMock('../../../../../components/detailed-list/detailed-list');
      const detailedListCall = DetailedListWrapper.mock.calls[0];
      const detailedListProps = detailedListCall[0];

      // Simulate item action click
      const testItem = { id: 'context-5', title: '@file5' };
      detailedListProps.onItemActionClick({}, testItem);

      expect(mockOnContextItemRemove).toHaveBeenCalled();
    });

    it('should handle item click with title instead of id', () => {
      promptTopBar = new PromptTopBar({
        tabId: 'test-tab',
        title: 'Test Title',
        contextItems: [
          {
            command: '@noId',
            description: 'Item without ID'
          }
        ],
        onContextItemRemove: mockOnContextItemRemove
      });

      promptTopBar.visibleCount = 0;
      promptTopBar.generateOverflowOverlayChildren();

      const { DetailedListWrapper } = jest.requireMock('../../../../../components/detailed-list/detailed-list');
      const detailedListCall = DetailedListWrapper.mock.calls[0];
      const detailedListProps = detailedListCall[0];

      // Simulate item click with title but no id
      const testItem = { title: '@noId' };
      detailedListProps.onItemClick(testItem);

      expect(mockOnContextItemRemove).toHaveBeenCalled();
    });

    it('should not remove item when item id and title are null', () => {
      promptTopBar = new PromptTopBar({
        tabId: 'test-tab',
        title: 'Test Title',
        contextItems: manyContextItems,
        onContextItemRemove: mockOnContextItemRemove
      });

      promptTopBar.visibleCount = 5;
      promptTopBar.generateOverflowOverlayChildren();

      const { DetailedListWrapper } = jest.requireMock('../../../../../components/detailed-list/detailed-list');
      const detailedListCall = DetailedListWrapper.mock.calls[0];
      const detailedListProps = detailedListCall[0];

      // Simulate item click with null item
      detailedListProps.onItemClick({ id: null, title: null });

      expect(mockOnContextItemRemove).not.toHaveBeenCalled();
    });
  });

  describe('Overflow Item Conversion', () => {
    it('should convert overflow items to detailed list group format', () => {
      promptTopBar = new PromptTopBar({
        tabId: 'test-tab',
        title: 'Test Title',
        contextItems: manyContextItems
      });

      promptTopBar.visibleCount = 5;

      const overflowItems = promptTopBar.getOverflowItemsAsDetailedListGroup();

      expect(overflowItems).toBeDefined();
      expect(overflowItems.length).toBeGreaterThan(0);

      // Check that items have remove actions
      const firstGroup = overflowItems[0];
      if ((firstGroup.children != null) && firstGroup.children.length > 0) {
        const firstChild = firstGroup.children[0];
        expect(firstChild.actions).toBeDefined();
        expect(firstChild.actions?.[0].id).toBe('remove');
      }
    });

    it('should handle empty overflow items', () => {
      // Mock the convertQuickActionCommandGroupsToDetailedListGroups function
      jest.mock('../../../../../helper/quick-pick-data-handler', () => ({
        convertQuickActionCommandGroupsToDetailedListGroups: jest.fn().mockReturnValue([])
      }));

      promptTopBar = new PromptTopBar({
        tabId: 'test-tab',
        title: 'Test Title',
        contextItems: []
      });

      // Mock the implementation to return an empty array
      jest.spyOn(promptTopBar, 'getOverflowContextItems').mockReturnValue([]);

      const overflowItems = promptTopBar.getOverflowItemsAsDetailedListGroup();

      // Since we're mocking the return value, we can assert it's an empty array
      expect(Array.isArray(overflowItems)).toBe(true);
    });
  });
});
