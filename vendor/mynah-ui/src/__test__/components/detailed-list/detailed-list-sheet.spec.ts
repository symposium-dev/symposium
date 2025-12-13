/*!
 * Copyright 2022 Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

import { DetailedListSheet, DetailedListSheetProps } from '../../../components/detailed-list/detailed-list-sheet';
import { DetailedList, ChatItemButton, MynahEventNames } from '../../../static';
import { MynahUIGlobalEvents } from '../../../helper/events';
import { MynahIcons } from '../../../components/icon';

// Mock the global events
jest.mock('../../../helper/events', () => ({
  MynahUIGlobalEvents: {
    getInstance: jest.fn(() => ({
      dispatch: jest.fn()
    }))
  }
}));

// Mock the detailed list wrapper
jest.mock('../../../components/detailed-list/detailed-list', () => ({
  DetailedListWrapper: jest.fn().mockImplementation(() => ({
    render: document.createElement('div'),
    update: jest.fn()
  }))
}));

describe('DetailedListSheet Component', () => {
  let detailedListSheet: DetailedListSheet;
  let mockDispatch: jest.Mock;
  let mockOnFilterValueChange: jest.Mock;
  let mockOnKeyPress: jest.Mock;
  let mockOnItemSelect: jest.Mock;
  let mockOnItemClick: jest.Mock;
  let mockOnBackClick: jest.Mock;
  let mockOnTitleActionClick: jest.Mock;
  let mockOnActionClick: jest.Mock;
  let mockOnFilterActionClick: jest.Mock;
  let mockOnClose: jest.Mock;

  const basicDetailedList: DetailedList = {
    header: {
      title: 'Test Sheet',
      description: 'Test sheet description',
      icon: MynahIcons.INFO
    },
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
  };

  const detailedListWithActions: DetailedList = {
    header: {
      title: 'Sheet with Actions',
      description: 'Sheet with header actions',
      actions: [
        {
          id: 'header-action-1',
          text: 'Header Action',
          icon: MynahIcons.PENCIL
        }
      ]
    },
    list: basicDetailedList.list
  };

  const detailedListWithStatus: DetailedList = {
    header: {
      title: 'Sheet with Status',
      description: 'Sheet with status',
      status: {
        status: 'warning',
        title: 'Warning Status',
        description: 'This is a warning',
        icon: MynahIcons.WARNING
      }
    },
    list: basicDetailedList.list
  };

  beforeEach(() => {
    mockDispatch = jest.fn();
    mockOnFilterValueChange = jest.fn();
    mockOnKeyPress = jest.fn();
    mockOnItemSelect = jest.fn();
    mockOnItemClick = jest.fn();
    mockOnBackClick = jest.fn();
    mockOnTitleActionClick = jest.fn();
    mockOnActionClick = jest.fn();
    mockOnFilterActionClick = jest.fn();
    mockOnClose = jest.fn();

    (MynahUIGlobalEvents.getInstance as jest.Mock).mockReturnValue({
      dispatch: mockDispatch
    });

    jest.clearAllMocks();

    // Mock window event listeners
    global.addEventListener = jest.fn();
    global.removeEventListener = jest.fn();
  });

  afterEach(() => {
    jest.clearAllMocks();
  });

  describe('Basic Functionality', () => {
    it('should create detailed list sheet with basic props', () => {
      detailedListSheet = new DetailedListSheet({ detailedList: basicDetailedList });

      expect(detailedListSheet).toBeDefined();
      expect(detailedListSheet.props).toBeDefined();
      expect(detailedListSheet.detailedListWrapper).toBeDefined();
    });

    it('should create detailed list wrapper without header', () => {
      detailedListSheet = new DetailedListSheet({ detailedList: basicDetailedList });

      const { DetailedListWrapper } = jest.requireMock('../../../components/detailed-list/detailed-list');
      expect(DetailedListWrapper).toHaveBeenCalledWith(
        expect.objectContaining({
          detailedList: expect.objectContaining({
            header: undefined
          })
        })
      );
    });

    it('should pass event handlers to detailed list wrapper', () => {
      const props: DetailedListSheetProps = {
        detailedList: basicDetailedList,
        events: {
          onFilterValueChange: mockOnFilterValueChange,
          onItemSelect: mockOnItemSelect,
          onItemClick: mockOnItemClick,
          onActionClick: mockOnActionClick,
          onFilterActionClick: mockOnFilterActionClick
        }
      };

      detailedListSheet = new DetailedListSheet(props);

      const { DetailedListWrapper } = jest.requireMock('../../../components/detailed-list/detailed-list');
      expect(DetailedListWrapper).toHaveBeenCalledWith(
        expect.objectContaining({
          onFilterValueChange: mockOnFilterValueChange,
          onItemSelect: mockOnItemSelect,
          onItemClick: mockOnItemClick,
          onItemActionClick: mockOnActionClick,
          onFilterActionClick: mockOnFilterActionClick
        })
      );
    });

    it('should store props reference', () => {
      const props: DetailedListSheetProps = {
        detailedList: basicDetailedList,
        events: {
          onKeyPress: mockOnKeyPress
        }
      };

      detailedListSheet = new DetailedListSheet(props);

      expect(detailedListSheet.props).toBe(props);
    });
  });

  describe('Sheet Opening', () => {
    it('should dispatch open sheet event', () => {
      detailedListSheet = new DetailedListSheet({ detailedList: basicDetailedList });

      detailedListSheet.open();

      expect(mockDispatch).toHaveBeenCalledWith(MynahEventNames.OPEN_SHEET, expect.objectContaining({
        fullScreen: true,
        title: basicDetailedList.header?.title,
        description: basicDetailedList.header?.description,
        children: expect.any(Array)
      }));
    });

    it('should dispatch open sheet with back button when requested', () => {
      detailedListSheet = new DetailedListSheet({ detailedList: basicDetailedList });

      detailedListSheet.open(true);

      expect(mockDispatch).toHaveBeenCalledWith(MynahEventNames.OPEN_SHEET, expect.objectContaining({
        showBackButton: true
      }));
    });

    it('should dispatch open sheet with header actions', () => {
      detailedListSheet = new DetailedListSheet({ detailedList: detailedListWithActions });

      detailedListSheet.open();

      expect(mockDispatch).toHaveBeenCalledWith(MynahEventNames.OPEN_SHEET, expect.objectContaining({
        actions: detailedListWithActions.header?.actions
      }));
    });

    it('should dispatch open sheet with status', () => {
      detailedListSheet = new DetailedListSheet({ detailedList: detailedListWithStatus });

      detailedListSheet.open();

      expect(mockDispatch).toHaveBeenCalledWith(MynahEventNames.OPEN_SHEET, expect.objectContaining({
        status: detailedListWithStatus.header?.status
      }));
    });

    it('should add keydown event listener', () => {
      detailedListSheet = new DetailedListSheet({
        detailedList: basicDetailedList,
        events: { onKeyPress: mockOnKeyPress }
      });

      detailedListSheet.open();

      expect(global.addEventListener).toHaveBeenCalledWith('keydown', expect.any(Function));
    });

    it('should handle sheet close callback', () => {
      detailedListSheet = new DetailedListSheet({
        detailedList: basicDetailedList,
        events: { onClose: mockOnClose }
      });

      detailedListSheet.open();

      // Get the onClose callback from the dispatch call
      const dispatchCall = mockDispatch.mock.calls[0];
      const sheetProps = dispatchCall[1];

      // Simulate sheet close
      sheetProps.onClose();

      expect(mockOnClose).toHaveBeenCalled();
      expect(global.removeEventListener).toHaveBeenCalledWith('keydown', expect.any(Function));
    });

    it('should handle title action click', () => {
      detailedListSheet = new DetailedListSheet({
        detailedList: detailedListWithActions,
        events: { onTitleActionClick: mockOnTitleActionClick }
      });

      detailedListSheet.open();

      // Get the onActionClick callback from the dispatch call
      const dispatchCall = mockDispatch.mock.calls[0];
      const sheetProps = dispatchCall[1];

      const testAction: ChatItemButton = { id: 'test-action', text: 'Test' };
      sheetProps.onActionClick(testAction);

      expect(mockOnTitleActionClick).toHaveBeenCalledWith(testAction);
    });

    it('should handle back button click', () => {
      detailedListSheet = new DetailedListSheet({
        detailedList: basicDetailedList,
        events: { onBackClick: mockOnBackClick }
      });

      detailedListSheet.open();

      // Get the onBack callback from the dispatch call
      const dispatchCall = mockDispatch.mock.calls[0];
      const sheetProps = dispatchCall[1];

      sheetProps.onBack();

      expect(mockOnBackClick).toHaveBeenCalled();
    });
  });

  describe('Keyboard Event Handling', () => {
    it('should handle keydown events', () => {
      detailedListSheet = new DetailedListSheet({
        detailedList: basicDetailedList,
        events: { onKeyPress: mockOnKeyPress }
      });

      detailedListSheet.open();

      // Get the keydown handler from addEventListener call
      const addEventListenerCall = (global.addEventListener as jest.Mock).mock.calls.find(
        call => call[0] === 'keydown'
      );
      const keydownHandler = addEventListenerCall[1];

      const keyEvent = new KeyboardEvent('keydown', { key: 'Enter' });
      keydownHandler(keyEvent);

      expect(mockOnKeyPress).toHaveBeenCalledWith(keyEvent);
    });

    it('should not throw error when onKeyPress is not provided', () => {
      detailedListSheet = new DetailedListSheet({ detailedList: basicDetailedList });

      detailedListSheet.open();

      // Get the keydown handler
      const addEventListenerCall = (global.addEventListener as jest.Mock).mock.calls.find(
        call => call[0] === 'keydown'
      );
      const keydownHandler = addEventListenerCall[1];

      const keyEvent = new KeyboardEvent('keydown', { key: 'Escape' });

      // Should not throw error
      expect(() => keydownHandler(keyEvent)).not.toThrow();
    });
  });

  describe('Sheet Updates', () => {
    it('should update sheet header', () => {
      detailedListSheet = new DetailedListSheet({ detailedList: basicDetailedList });

      const updatedList: DetailedList = {
        header: {
          title: 'Updated Title',
          description: 'Updated description'
        }
      };

      detailedListSheet.update(updatedList);

      expect(mockDispatch).toHaveBeenCalledWith(MynahEventNames.UPDATE_SHEET, expect.objectContaining({
        title: 'Updated Title',
        description: 'Updated description'
      }));
    });

    it('should update sheet with back button', () => {
      detailedListSheet = new DetailedListSheet({ detailedList: basicDetailedList });

      const updatedList: DetailedList = {
        header: {
          title: 'Updated Title'
        }
      };

      detailedListSheet.update(updatedList, true);

      expect(mockDispatch).toHaveBeenCalledWith(MynahEventNames.UPDATE_SHEET, expect.objectContaining({
        showBackButton: true
      }));
    });

    it('should update sheet with status', () => {
      detailedListSheet = new DetailedListSheet({ detailedList: basicDetailedList });

      const updatedList: DetailedList = {
        header: {
          title: 'Updated Title',
          status: {
            status: 'error',
            title: 'Error Status',
            description: 'Something went wrong'
          }
        }
      };

      detailedListSheet.update(updatedList);

      expect(mockDispatch).toHaveBeenCalledWith(MynahEventNames.UPDATE_SHEET, expect.objectContaining({
        status: updatedList.header?.status
      }));
    });

    it('should update sheet with actions', () => {
      detailedListSheet = new DetailedListSheet({ detailedList: basicDetailedList });

      const updatedList: DetailedList = {
        header: {
          title: 'Updated Title',
          actions: [
            { id: 'new-action', text: 'New Action', icon: MynahIcons.REFRESH }
          ]
        }
      };

      detailedListSheet.update(updatedList);

      expect(mockDispatch).toHaveBeenCalledWith(MynahEventNames.UPDATE_SHEET, expect.objectContaining({
        actions: updatedList.header?.actions
      }));
    });

    it('should update detailed list wrapper', () => {
      detailedListSheet = new DetailedListSheet({ detailedList: basicDetailedList });

      const updatedList: DetailedList = {
        header: {
          title: 'Updated Title'
        },
        list: [
          {
            groupName: 'Updated Group',
            children: [
              { id: 'new-item', title: 'New Item' }
            ]
          }
        ]
      };

      detailedListSheet.update(updatedList);

      expect(detailedListSheet.detailedListWrapper.update).toHaveBeenCalledWith(
        expect.objectContaining({
          header: undefined, // Header should be removed for wrapper
          list: updatedList.list
        })
      );
    });

    it('should merge props when updating', () => {
      detailedListSheet = new DetailedListSheet({ detailedList: basicDetailedList });

      const partialUpdate: DetailedList = {
        list: [
          {
            groupName: 'New Group',
            children: [ { id: 'new-item', title: 'New Item' } ]
          }
        ]
      };

      detailedListSheet.update(partialUpdate);

      // Should merge with existing props
      expect(detailedListSheet.props.detailedList.header?.title).toBe(basicDetailedList.header?.title);
      expect(detailedListSheet.props.detailedList.list).toBe(partialUpdate.list);
    });

    it('should not dispatch update sheet when header is not provided', () => {
      detailedListSheet = new DetailedListSheet({ detailedList: basicDetailedList });

      const updatedList: DetailedList = {
        list: [
          {
            groupName: 'Updated Group',
            children: [ { id: 'item', title: 'Item' } ]
          }
        ]
      };

      detailedListSheet.update(updatedList);

      // Should only call wrapper update, not sheet update
      expect(mockDispatch).not.toHaveBeenCalledWith(MynahEventNames.UPDATE_SHEET, expect.anything());
    });

    it('should handle update with null header', () => {
      detailedListSheet = new DetailedListSheet({ detailedList: basicDetailedList });

      const updatedList: DetailedList = {
        header: null as any,
        list: [
          {
            groupName: 'Updated Group',
            children: [ { id: 'item', title: 'Item' } ]
          }
        ]
      };

      detailedListSheet.update(updatedList);

      // Should not dispatch sheet update for null header
      expect(mockDispatch).not.toHaveBeenCalledWith(MynahEventNames.UPDATE_SHEET, expect.anything());
    });

    it('should handle update with undefined showBackButton', () => {
      detailedListSheet = new DetailedListSheet({ detailedList: basicDetailedList });

      const updatedList: DetailedList = {
        header: {
          title: 'Updated Title'
        }
      };

      detailedListSheet.update(updatedList, undefined);

      expect(mockDispatch).toHaveBeenCalledWith(MynahEventNames.UPDATE_SHEET, expect.objectContaining({
        showBackButton: undefined
      }));
    });
  });

  describe('Sheet Closing', () => {
    it('should dispatch close sheet event', () => {
      detailedListSheet = new DetailedListSheet({ detailedList: basicDetailedList });

      detailedListSheet.close();

      expect(mockDispatch).toHaveBeenCalledWith(MynahEventNames.CLOSE_SHEET);
    });
  });

  describe('Edge Cases', () => {
    it('should handle detailed list without header', () => {
      const listWithoutHeader: DetailedList = {
        list: [
          {
            groupName: 'Group',
            children: [ { id: 'item', title: 'Item' } ]
          }
        ]
      };

      detailedListSheet = new DetailedListSheet({ detailedList: listWithoutHeader });

      detailedListSheet.open();

      expect(mockDispatch).toHaveBeenCalledWith(MynahEventNames.OPEN_SHEET, expect.objectContaining({
        title: undefined,
        description: undefined
      }));
    });

    it('should handle empty events object', () => {
      detailedListSheet = new DetailedListSheet({
        detailedList: basicDetailedList,
        events: {}
      });

      detailedListSheet.open();

      // Should not throw error
      expect(detailedListSheet).toBeDefined();
    });

    it('should handle undefined events', () => {
      detailedListSheet = new DetailedListSheet({ detailedList: basicDetailedList });

      detailedListSheet.open();

      // Should not throw error
      expect(detailedListSheet).toBeDefined();
    });

    it('should handle update without header', () => {
      detailedListSheet = new DetailedListSheet({ detailedList: basicDetailedList });

      const updatedList: DetailedList = {
        list: [ { groupName: 'Group', children: [] } ]
      };

      detailedListSheet.update(updatedList);

      // Should only update wrapper
      expect(detailedListSheet.detailedListWrapper.update).toHaveBeenCalled();
    });

    it('should handle empty update', () => {
      detailedListSheet = new DetailedListSheet({ detailedList: basicDetailedList });

      detailedListSheet.update({});

      // Should not throw error
      expect(detailedListSheet).toBeDefined();
    });
  });

  describe('Legacy Support', () => {
    it('should handle tabId prop for backwards compatibility', () => {
      const propsWithTabId: DetailedListSheetProps = {
        tabId: 'legacy-tab-id',
        detailedList: basicDetailedList
      };

      detailedListSheet = new DetailedListSheet(propsWithTabId);

      expect(detailedListSheet.props.tabId).toBe('legacy-tab-id');
    });
  });

  describe('Event Handler Integration', () => {
    it('should handle all event types', () => {
      const allEvents: DetailedListSheetProps['events'] = {
        onFilterValueChange: mockOnFilterValueChange,
        onKeyPress: mockOnKeyPress,
        onItemSelect: mockOnItemSelect,
        onItemClick: mockOnItemClick,
        onBackClick: mockOnBackClick,
        onTitleActionClick: mockOnTitleActionClick,
        onActionClick: mockOnActionClick,
        onFilterActionClick: mockOnFilterActionClick,
        onClose: mockOnClose
      };

      detailedListSheet = new DetailedListSheet({
        detailedList: detailedListWithActions,
        events: allEvents
      });

      detailedListSheet.open();

      // Verify all handlers are properly set up
      const { DetailedListWrapper } = jest.requireMock('../../../components/detailed-list/detailed-list');
      expect(DetailedListWrapper).toHaveBeenCalledWith(
        expect.objectContaining({
          onFilterValueChange: mockOnFilterValueChange,
          onItemSelect: mockOnItemSelect,
          onItemClick: mockOnItemClick,
          onItemActionClick: mockOnActionClick,
          onFilterActionClick: mockOnFilterActionClick
        })
      );

      expect(global.addEventListener).toHaveBeenCalledWith('keydown', expect.any(Function));
    });
  });
});
