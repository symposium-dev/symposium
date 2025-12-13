/*!
 * Copyright 2022 Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

import { TopBarButton, TopBarButtonOverlayProps } from '../../../../../components/chat-item/prompt-input/prompt-top-bar/top-bar-button';
import { ChatItemButton, DetailedList } from '../../../../../static';
import { MynahIcons } from '../../../../../components/icon';

// Mock the overlay component
const mockUpdateContent = jest.fn();
const mockClose = jest.fn();

jest.mock('../../../../../components/overlay', () => ({
  Overlay: jest.fn().mockImplementation(() => ({
    close: mockClose,
    updateContent: mockUpdateContent
  })),
  OverlayHorizontalDirection: {
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

describe('TopBarButton Overlay Functionality', () => {
  let topBarButton: TopBarButton;
  let mockOnKeyPress: jest.Mock;
  let mockOnGroupClick: jest.Mock;
  let mockOnItemClick: jest.Mock;
  let mockOnClose: jest.Mock;

  const basicButton: ChatItemButton = {
    id: 'test-button',
    text: 'Test Button',
    icon: MynahIcons.PLUS
  };

  const basicOverlayData: TopBarButtonOverlayProps = {
    tabId: 'test-tab',
    topBarButtonOverlay: {
      list: [
        {
          groupName: 'Test Group',
          children: [
            {
              id: 'item-1',
              title: 'Test Item 1',
              description: 'Description 1'
            }
          ]
        }
      ]
    }
  };

  beforeEach(() => {
    document.body.innerHTML = '';
    mockOnKeyPress = jest.fn();
    mockOnGroupClick = jest.fn();
    mockOnItemClick = jest.fn();
    mockOnClose = jest.fn();
    jest.clearAllMocks();

    // Mock window event listeners
    global.addEventListener = jest.fn();
    global.removeEventListener = jest.fn();
  });

  afterEach(() => {
    document.body.innerHTML = '';
    jest.clearAllMocks();
  });

  describe('Overlay Creation and Updates', () => {
    it('should create overlay with event handlers', () => {
      topBarButton = new TopBarButton({ topBarButton: basicButton });

      const overlayData: TopBarButtonOverlayProps = {
        ...basicOverlayData,
        events: {
          onKeyPress: mockOnKeyPress,
          onGroupClick: mockOnGroupClick,
          onItemClick: mockOnItemClick,
          onClose: mockOnClose
        }
      };

      topBarButton.showOverlay(overlayData);

      const { Overlay } = jest.requireMock('../../../../../components/overlay');
      expect(Overlay).toHaveBeenCalled();
      expect(global.addEventListener).toHaveBeenCalledWith('keydown', expect.any(Function));
    });

    it('should update existing overlay content', () => {
      topBarButton = new TopBarButton({ topBarButton: basicButton });

      // Show overlay first time
      topBarButton.showOverlay(basicOverlayData);

      // Show overlay second time (should update existing)
      const updatedOverlayData: TopBarButtonOverlayProps = {
        ...basicOverlayData,
        topBarButtonOverlay: {
          list: [
            {
              groupName: 'Updated Group',
              children: [
                { id: 'updated-item', title: 'Updated Item' }
              ]
            }
          ]
        }
      };

      topBarButton.showOverlay(updatedOverlayData);

      expect(mockUpdateContent).toHaveBeenCalled();
    });

    it('should handle overlay close callback', () => {
      topBarButton = new TopBarButton({ topBarButton: basicButton });

      const overlayData: TopBarButtonOverlayProps = {
        ...basicOverlayData,
        events: {
          onClose: mockOnClose
        }
      };

      topBarButton.showOverlay(overlayData);

      const overlayCall = jest.requireMock('../../../../../components/overlay').Overlay.mock.calls[0];
      const overlayProps = overlayCall[0];

      // Simulate overlay close
      overlayProps.onClose();

      expect(mockOnClose).toHaveBeenCalled();
      expect(global.removeEventListener).toHaveBeenCalledWith('keydown', expect.any(Function));
    });
  });

  describe('Detailed List Wrapper', () => {
    it('should create detailed list wrapper on first call', () => {
      topBarButton = new TopBarButton({ topBarButton: basicButton });

      const overlayData: TopBarButtonOverlayProps = {
        ...basicOverlayData,
        events: {
          onGroupClick: mockOnGroupClick,
          onItemClick: mockOnItemClick
        }
      };

      topBarButton.showOverlay(overlayData);

      const { DetailedListWrapper } = jest.requireMock('../../../../../components/detailed-list/detailed-list');
      expect(DetailedListWrapper).toHaveBeenCalled();
    });

    it('should update detailed list wrapper on subsequent calls', () => {
      topBarButton = new TopBarButton({ topBarButton: basicButton });

      // First call to create the wrapper
      topBarButton.showOverlay(basicOverlayData);

      // Set up the overlayData property manually for testing
      (topBarButton as any).overlayData = basicOverlayData;
      (topBarButton as any).checklistSelectorContainer = {
        update: mockDetailedListUpdate
      };

      // Call getItemGroups directly to trigger the update path
      (topBarButton as any).getItemGroups();

      expect(mockDetailedListUpdate).toHaveBeenCalled();
    });

    it('should handle group action click with null group name', () => {
      topBarButton = new TopBarButton({ topBarButton: basicButton });

      const overlayData: TopBarButtonOverlayProps = {
        ...basicOverlayData,
        events: {
          onGroupClick: mockOnGroupClick
        }
      };

      topBarButton.showOverlay(overlayData);

      const { DetailedListWrapper } = jest.requireMock('../../../../../components/detailed-list/detailed-list');
      const detailedListCall = DetailedListWrapper.mock.calls[0];
      const detailedListProps = detailedListCall[0];

      // Simulate group action click with null group name
      detailedListProps.onGroupActionClick({}, null);

      expect(mockOnGroupClick).not.toHaveBeenCalled();
    });

    it('should handle item action click with null item', () => {
      topBarButton = new TopBarButton({ topBarButton: basicButton });

      const overlayData: TopBarButtonOverlayProps = {
        ...basicOverlayData,
        events: {
          onItemClick: mockOnItemClick
        }
      };

      topBarButton.showOverlay(overlayData);

      const { DetailedListWrapper } = jest.requireMock('../../../../../components/detailed-list/detailed-list');
      const detailedListCall = DetailedListWrapper.mock.calls[0];
      const detailedListProps = detailedListCall[0];

      // Simulate item action click with null item
      detailedListProps.onItemActionClick({}, null);

      expect(mockOnItemClick).not.toHaveBeenCalled();
    });
  });

  describe('Overlay Content Updates', () => {
    it('should update overlay content when topBarButtonOverlay changes', () => {
      topBarButton = new TopBarButton({ topBarButton: basicButton });

      // Show overlay first
      topBarButton.showOverlay(basicOverlayData);

      // Set up the overlay property manually for testing
      (topBarButton as any).overlay = { updateContent: mockUpdateContent };

      const updatedOverlay: DetailedList = {
        list: [
          {
            groupName: 'New Group',
            children: [
              { id: 'new-item', title: 'New Item' }
            ]
          }
        ]
      };

      topBarButton.onTopBarButtonOverlayChanged(updatedOverlay);

      expect(mockUpdateContent).toHaveBeenCalled();
    });

    it('should not update overlay content when overlay is not shown', () => {
      topBarButton = new TopBarButton({ topBarButton: basicButton });

      // Set up the overlayData property manually for testing
      (topBarButton as any).overlayData = basicOverlayData;

      const updatedOverlay: DetailedList = {
        list: [
          {
            groupName: 'New Group',
            children: [
              { id: 'new-item', title: 'New Item' }
            ]
          }
        ]
      };

      // Should not throw error when overlay is not shown
      expect(() => topBarButton.onTopBarButtonOverlayChanged(updatedOverlay)).not.toThrow();
    });
  });
});
